//! POSIX `<search.h>` — search and data structure operations.
//!
//! Implements all standard POSIX `<search.h>` functions:
//!
//! - **Binary search tree**: `tsearch`, `tfind`, `tdelete`, `twalk`,
//!   and the glibc extension `tdestroy`.
//! - **Hash table**: `hcreate`, `hdestroy`, `hsearch` (global hash
//!   table with separate chaining).
//! - **Linear search**: `lsearch` (search + insert), `lfind`
//!   (search only).
//! - **Linked list**: `insque` (insert), `remque` (remove) for
//!   doubly-linked lists.
//!
//! ## BST Design
//!
//! - A red-black tree, glibc 2.39's (misc/tsearch.c): insertion splits and
//!   rotates on the way down, deletion repairs on the way up, so every
//!   operation is O(log n) whatever order the keys come in.  It was a plain
//!   binary search tree until 2026-09-26, which keys inserted in order —
//!   the usual case — turned into a linked list.
//! - Nodes are allocated via `malloc` and freed via `free`.
//! - The comparison function has the standard C prototype:
//!   `int compar(const void *a, const void *b)`.
//! - `twalk` visits nodes in the POSIX-defined order: preorder,
//!   postorder (= in-order), endorder, and leaf.
//!
//! ## Hash Table Design
//!
//! - Single global hash table (POSIX spec only defines one table).
//! - Separate chaining with singly-linked bucket lists.
//! - FNV-1a hash on the key string bytes.
//! - `hcreate(nel)` allocates at least `nel` buckets.
//! - `hsearch(ENTER)` inserts if not found; `hsearch(FIND)` never inserts.

// `crate::malloc::malloc` returns memory aligned for any standard type
// (POSIX/C guarantee: at least `_Alignof(max_align_t)`), so casting its
// `*mut u8` return to `*mut Node` / `*mut HashNode` / `*mut QueueEntry`
// is always safe.  clippy cannot see through `malloc` to verify this,
// so allow the lint at module scope rather than annotating every cast.
#![allow(clippy::cast_ptr_alignment)]

use crate::errno;

// ---------------------------------------------------------------------------
// Node layout
// ---------------------------------------------------------------------------

/// A node of the `tsearch` tree: a red-black tree, as glibc's is
/// (misc/tsearch.c), so that insertion, lookup and deletion cost O(log n)
/// whatever order the keys arrive in.
///
/// `key` must stay the first field: `tsearch` and `tfind` return a pointer to
/// the node, which the caller reads as a pointer to a pointer to its key.
/// glibc keeps the colour in the low bit of `left`; a separate field costs a
/// word per node and no pointer arithmetic.
#[repr(C)]
struct Node {
    /// Pointer to user data.
    key: *const u8,
    /// Left child (keys less than this node).
    left: *mut Node,
    /// Right child (keys greater than this node).
    right: *mut Node,
    /// Red, or black.  A null child counts as black.
    red: bool,
}

/// Comparison function type.
pub type ComparFn = extern "C" fn(*const u8, *const u8) -> i32;

/// Action values passed to the `twalk` callback.
pub const PREORDER: i32 = 0;
/// Visit of an internal node during in-order traversal (2nd visit).
pub const POSTORDER: i32 = 1;
/// Visit of an internal node during post-order traversal (3rd visit).
pub const ENDORDER: i32 = 2;
/// Visit of a leaf node (only visit).
pub const LEAF: i32 = 3;

/// Action callback type for `twalk`.
///
/// Arguments: `(node_ptr, visit_order, depth)`.
pub type TwalkFn = extern "C" fn(*const u8, i32, i32);

/// Action callback type for `twalk_r`.
///
/// Arguments: `(node_ptr, visit_order, closure)`.
pub type TwalkRFn = extern "C" fn(*const u8, i32, *mut u8);

/// Free callback type for `tdestroy`.
pub type TdestroyFn = extern "C" fn(*mut u8);

/// The most nodes `tdelete` can have above the one it removes.  A red-black
/// tree of n nodes is at most `2 * log2(n + 1)` high, so 128 covers any tree
/// an address space can hold; glibc grows its stack instead, which a valid
/// tree never needs.
const MAX_TREE_HEIGHT: usize = 128;

/// Red, for a node that may be null (a null child is black).
///
/// # Safety
///
/// `n` must be null or a live node.
unsafe fn is_red(n: *mut Node) -> bool {
    // SAFETY: the caller's contract.
    !n.is_null() && unsafe { (*n).red }
}

/// glibc's `maybe_split_for_insert`: on the way down, split a node with two
/// red children (it turns red, they turn black), and repair two red edges in
/// a row with one or two rotations.  `rootp` points at the lowest node
/// visited, `parentp` and `gparentp` at its parent and grandparent (either
/// may be null); `p_r` and `gp_r` are the comparisons that chose the way to
/// `rootp`.  `force` skips the two-red-children test: the node just inserted
/// is to be treated as split.
///
/// # Safety
///
/// Every non-null pointer must point at a live link of the tree, `*rootp`
/// must be a live node, and `gparentp` must be non-null whenever `parentp`
/// is and `*parentp` is red — which a valid tree guarantees, a red node never
/// being the root.
unsafe fn maybe_split_for_insert(
    rootp: *mut *mut Node,
    parentp: *mut *mut Node,
    gparentp: *mut *mut Node,
    p_r: i32,
    gp_r: i32,
    force: bool,
) {
    // SAFETY: the caller's contract covers every dereference below; the
    // rotations only relink nodes that are already in the tree.
    unsafe {
        let root = *rootp;
        let rp: *mut *mut Node = &raw mut (*root).right;
        let rpn = (*root).right;
        let lp: *mut *mut Node = &raw mut (*root).left;
        let lpn = (*root).left;

        if !(force || (is_red(rpn) && is_red(lpn))) {
            return;
        }
        // This node becomes red, its children black.
        (*root).red = true;
        if !rpn.is_null() {
            (*rpn).red = false;
        }
        if !lpn.is_null() {
            (*lpn).red = false;
        }

        // If the parent is red too, rotate.
        if parentp.is_null() || !is_red(*parentp) || gparentp.is_null() {
            return;
        }
        let gp = *gparentp;
        let p = *parentp;
        if (p_r > 0) != (gp_r > 0) {
            // The two red edges bend: the child goes to the top, with its
            // parent and grandparent as its children.
            (*p).red = true;
            (*gp).red = true;
            (*root).red = false;
            if p_r < 0 {
                // Child is left of parent.
                (*p).left = rpn;
                *rp = p;
                (*gp).right = lpn;
                *lp = gp;
            } else {
                // Child is right of parent.
                (*p).right = lpn;
                *lp = p;
                (*gp).left = rpn;
                *rp = gp;
            }
            *gparentp = root;
        } else {
            // Both edges go the same way: the parent goes to the top, with
            // the grandparent and the child as its children.
            *gparentp = p;
            (*p).red = false;
            (*gp).red = true;
            if p_r < 0 {
                (*gp).left = (*p).right;
                (*p).right = gp;
            } else {
                (*gp).right = (*p).left;
                (*p).left = gp;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// tsearch — insert or find a node
// ---------------------------------------------------------------------------

/// `tsearch` — find `key` in the tree at `*rootp`, inserting it if absent.
///
/// Returns the node holding the key (read it as a pointer to a pointer to
/// the key), or null if `rootp` is null or a new node cannot be allocated.
/// glibc 2.39's top-down red-black insertion (misc/tsearch.c), so a tree
/// built from keys in order stays O(log n) high; until 2026-09-26 this was an
/// unbalanced tree, which sorted input made a linked list.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tsearch(key: *const u8, rootp: *mut *mut u8, compar: ComparFn) -> *mut u8 {
    if rootp.is_null() {
        return core::ptr::null_mut();
    }
    let mut rootp: *mut *mut Node = rootp.cast();
    let mut parentp: *mut *mut Node = core::ptr::null_mut();
    let mut gparentp: *mut *mut Node = core::ptr::null_mut();
    let (mut r, mut p_r, mut gp_r) = (0i32, 0i32, 0i32);

    // SAFETY: `rootp` is the caller's root link, and every link followed
    // below is a child field of a live node of the tree it roots.
    unsafe {
        // The root is always black; this saves tests below.
        let root = *rootp;
        if !root.is_null() {
            (*root).red = false;
        }

        let mut nextp = rootp;
        while !(*nextp).is_null() {
            let root = *rootp;
            r = compar(key, (*root).key);
            if r == 0 {
                return root.cast();
            }
            maybe_split_for_insert(rootp, parentp, gparentp, p_r, gp_r, false);
            // If that rotated, `parentp` and `gparentp` are stale; glibc's
            // comment: they are never used again in that case.
            nextp = if r < 0 {
                &raw mut (*root).left
            } else {
                &raw mut (*root).right
            };
            if (*nextp).is_null() {
                break;
            }
            gparentp = parentp;
            parentp = rootp;
            rootp = nextp;
            gp_r = p_r;
            p_r = r;
        }

        let q = crate::malloc::malloc(core::mem::size_of::<Node>()).cast::<Node>();
        if q.is_null() {
            errno::set_errno(errno::ENOMEM);
            return core::ptr::null_mut();
        }
        q.write(Node {
            key,
            left: core::ptr::null_mut(),
            right: core::ptr::null_mut(),
            red: true,
        });
        *nextp = q;
        if nextp != rootp {
            // Two red edges in a row are possible now; rotate them away.
            maybe_split_for_insert(nextp, rootp, parentp, r, p_r, true);
        }
        q.cast()
    }
}

/// `tfind` — find `key` in the tree at `*rootp`, without inserting.
///
/// Returns the node holding it, or null if it is absent or `rootp` is null.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tfind(key: *const u8, rootp: *const *mut u8, compar: ComparFn) -> *const u8 {
    if rootp.is_null() {
        return core::ptr::null();
    }
    // SAFETY: `rootp` is the caller's root link; each node reached is live.
    unsafe {
        let mut node = (*rootp).cast::<Node>();
        while !node.is_null() {
            let r = compar(key, (*node).key);
            if r == 0 {
                return node.cast_const().cast();
            }
            node = if r < 0 { (*node).left } else { (*node).right };
        }
    }
    core::ptr::null()
}

// ---------------------------------------------------------------------------
// tdelete — remove a node
// ---------------------------------------------------------------------------

/// `tdelete` — remove `key` from the tree at `*rootp`.
///
/// Returns the parent of the node that held the key, or null if the key is
/// absent or `rootp` is null.  When the key was at the root, POSIX leaves the
/// result unspecified but non-null; glibc and musl return the root node —
/// freed, if it was the one unchained — and this returns `rootp` itself,
/// which is non-null and not freed memory.  (Until 2026-09-26 it returned the
/// new root, which is null once the last node goes: success that read as
/// "not found".)
///
/// glibc 2.39's deletion: the node's key is overwritten with its in-order
/// successor's, the successor is unchained, and a black node lost is repaired
/// on the way back up.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tdelete(key: *const u8, rootp: *mut *mut u8, compar: ComparFn) -> *mut u8 {
    if rootp.is_null() {
        return core::ptr::null_mut();
    }
    let vrootp = rootp;
    let mut rootp: *mut *mut Node = rootp.cast();
    // The links above the node being removed, root first.
    let mut stack: [*mut *mut Node; MAX_TREE_HEIGHT] = [core::ptr::null_mut(); MAX_TREE_HEIGHT];
    let mut sp: usize = 0;

    // SAFETY: `rootp` is the caller's root link; every link on `stack` is a
    // child field of a live node of the tree, and every node dereferenced is
    // live.  Rotations and relinks only move nodes that are in the tree.
    unsafe {
        let mut root = *rootp;
        if root.is_null() {
            return core::ptr::null_mut();
        }
        let mut p = root;
        loop {
            let cmp = compar(key, (*root).key);
            if cmp == 0 {
                break;
            }
            let Some(slot) = stack.get_mut(sp) else {
                // Deeper than any valid tree can be.
                return core::ptr::null_mut();
            };
            *slot = rootp;
            sp = sp.wrapping_add(1);
            p = *rootp;
            if cmp < 0 {
                rootp = &raw mut (*p).left;
                root = (*p).left;
            } else {
                rootp = &raw mut (*p).right;
                root = (*p).right;
            }
            if root.is_null() {
                return core::ptr::null_mut();
            }
        }
        // `p` is the parent of the node holding the key -- or that node
        // itself, when it is the root.
        let retval: *mut u8 = if sp == 0 { vrootp.cast() } else { p.cast() };

        // The node that holds the key is not unchained unless it has at most
        // one child; otherwise its successor's key replaces its own, and the
        // successor -- which has no left child -- is unchained instead.
        let root = *rootp;
        let unchained = if (*root).left.is_null() || (*root).right.is_null() {
            root
        } else {
            let mut parentp = rootp;
            let mut up: *mut *mut Node = &raw mut (*root).right;
            loop {
                let Some(slot) = stack.get_mut(sp) else {
                    return core::ptr::null_mut();
                };
                *slot = parentp;
                sp = sp.wrapping_add(1);
                parentp = up;
                let upn = *up;
                if (*upn).left.is_null() {
                    break;
                }
                up = &raw mut (*upn).left;
            }
            *up
        };

        // One child of `unchained` is null; the other, `r`, takes its place.
        let mut r = (*unchained).left;
        if r.is_null() {
            r = (*unchained).right;
        }
        if sp == 0 {
            *rootp = r;
        } else if let Some(&link) = stack.get(sp.wrapping_sub(1)) {
            let q = *link;
            if unchained == (*q).right {
                (*q).right = r;
            } else {
                (*q).left = r;
            }
        }
        if unchained != root {
            (*root).key = (*unchained).key;
        }

        if !(*unchained).red {
            // A black edge is gone, so paths through `r` are one black node
            // short.  Repair upwards; null counts as black throughout.
            while sp > 0 && !is_red(r) {
                let Some(&pp) = stack.get(sp.wrapping_sub(1)) else {
                    break;
                };
                let mut pp = pp;
                let p = *pp;
                if r == (*p).left {
                    // Q is R's sibling, P their parent.
                    let mut q = (*p).right;
                    if is_red(q) {
                        // Rotate P left, so that Q is black below.
                        (*q).red = false;
                        (*p).red = true;
                        (*p).right = (*q).left;
                        (*q).left = p;
                        *pp = q;
                        pp = &raw mut (*q).left;
                        if let Some(slot) = stack.get_mut(sp) {
                            *slot = pp;
                        }
                        sp = sp.wrapping_add(1);
                        q = (*p).right;
                    }
                    // Q is black, and not null.
                    if !is_red((*q).left) && !is_red((*q).right) {
                        // Q's children are black: colour Q red and move up.
                        (*q).red = true;
                        r = p;
                    } else {
                        if !is_red((*q).right) {
                            // Q's left child Q2 is red: it goes to the top.
                            let q2 = (*q).left;
                            (*q2).red = (*p).red;
                            (*p).right = (*q2).left;
                            (*q).left = (*q2).right;
                            (*q2).right = q;
                            (*q2).left = p;
                            *pp = q2;
                            (*p).red = false;
                        } else {
                            // Q's right child is red: rotate P left.
                            (*q).red = (*p).red;
                            (*p).red = false;
                            (*(*q).right).red = false;
                            (*p).right = (*q).left;
                            (*q).left = p;
                            *pp = q;
                        }
                        // Repaired.
                        sp = 1;
                        r = core::ptr::null_mut();
                    }
                } else {
                    // The mirror image.
                    let mut q = (*p).left;
                    if is_red(q) {
                        (*q).red = false;
                        (*p).red = true;
                        (*p).left = (*q).right;
                        (*q).right = p;
                        *pp = q;
                        pp = &raw mut (*q).right;
                        if let Some(slot) = stack.get_mut(sp) {
                            *slot = pp;
                        }
                        sp = sp.wrapping_add(1);
                        q = (*p).left;
                    }
                    if !is_red((*q).right) && !is_red((*q).left) {
                        (*q).red = true;
                        r = p;
                    } else {
                        if !is_red((*q).left) {
                            let q2 = (*q).right;
                            (*q2).red = (*p).red;
                            (*p).left = (*q2).right;
                            (*q).right = (*q2).left;
                            (*q2).left = q;
                            (*q2).right = p;
                            *pp = q2;
                            (*p).red = false;
                        } else {
                            (*q).red = (*p).red;
                            (*p).red = false;
                            (*(*q).left).red = false;
                            (*p).left = (*q).right;
                            (*q).right = p;
                            *pp = q;
                        }
                        sp = 1;
                        r = core::ptr::null_mut();
                    }
                }
                sp = sp.wrapping_sub(1);
            }
            if !r.is_null() {
                (*r).red = false;
            }
        }

        crate::malloc::free(unchained.cast::<u8>());
        retval
    }
}

// ---------------------------------------------------------------------------
// twalk — walk the tree
// ---------------------------------------------------------------------------

/// Visit `node` and its subtree in `twalk`'s order: a leaf once, as
/// [`LEAF`]; any other node three times — [`PREORDER`] before its left
/// subtree, [`POSTORDER`] between, [`ENDORDER`] after its right.
///
/// # Safety
///
/// `node` must be a live node of a valid tree.
unsafe fn walk_subtree(node: *mut Node, visit: &mut dyn FnMut(*const u8, i32, i32), depth: i32) {
    // SAFETY: the caller's contract; the children of a live node are null or
    // live.  The recursion is as deep as the tree is high: O(log n).
    unsafe {
        let (left, right) = ((*node).left, (*node).right);
        let n = node.cast_const().cast::<u8>();
        if left.is_null() && right.is_null() {
            visit(n, LEAF, depth);
            return;
        }
        visit(n, PREORDER, depth);
        if !left.is_null() {
            walk_subtree(left, visit, depth.saturating_add(1));
        }
        visit(n, POSTORDER, depth);
        if !right.is_null() {
            walk_subtree(right, visit, depth.saturating_add(1));
        }
        visit(n, ENDORDER, depth);
    }
}

/// `twalk` — walk the tree rooted at `root`, calling `action` with each node,
/// its visit ([`PREORDER`], [`POSTORDER`], [`ENDORDER`], [`LEAF`]) and its
/// depth.  A null root or a null `action` walks nothing, as in glibc.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn twalk(root: *const u8, action: Option<TwalkFn>) {
    let Some(action) = action else {
        return;
    };
    if root.is_null() {
        return;
    }
    // SAFETY: a non-null `root` is the root of a tree built by `tsearch`.
    unsafe {
        walk_subtree(root.cast_mut().cast(), &mut |n, v, d| action(n, v, d), 0);
    }
}

/// `twalk_r` — as [`twalk`], passing `closure` to `action` where `twalk`
/// passes the depth (glibc 2.30).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn twalk_r(root: *const u8, action: Option<TwalkRFn>, closure: *mut u8) {
    let Some(action) = action else {
        return;
    };
    if root.is_null() {
        return;
    }
    // SAFETY: as for `twalk`.
    unsafe {
        walk_subtree(
            root.cast_mut().cast(),
            &mut |n, v, _| action(n, v, closure),
            0,
        );
    }
}

// ---------------------------------------------------------------------------
// tdestroy — destroy the entire tree
// ---------------------------------------------------------------------------

/// Free `node`'s subtree, children first, calling `free_fn` on each key.
///
/// # Safety
///
/// `node` must be null or a live node of a valid tree, which this frees.
unsafe fn destroy_subtree(node: *mut Node, free_fn: Option<TdestroyFn>) {
    if node.is_null() {
        return;
    }
    // SAFETY: the caller's contract; recursion is O(log n) deep.
    unsafe {
        destroy_subtree((*node).left, free_fn);
        destroy_subtree((*node).right, free_fn);
        if let Some(f) = free_fn {
            f((*node).key.cast_mut());
        }
        crate::malloc::free(node.cast::<u8>());
    }
}

/// `tdestroy` — free every node of the tree rooted at `root`, calling
/// `free_fn` on each key first (glibc extension).
///
/// glibc calls `free_fn` unconditionally, so a null one crashes there; here
/// it frees the nodes and no keys, a null `extern "C" fn` being undefined
/// behaviour in Rust before it is ever called.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tdestroy(root: *mut u8, free_fn: Option<TdestroyFn>) {
    // SAFETY: `root` is null or the root of a tree built by `tsearch`, which
    // the caller hands over.
    unsafe { destroy_subtree(root.cast(), free_fn) };
}

// ===========================================================================
// Hash table (hcreate / hdestroy / hsearch)
// ===========================================================================

/// Hash table entry (public, matches POSIX `ENTRY` struct).
#[repr(C)]
pub struct Entry {
    /// Key string (NUL-terminated).
    pub key: *mut u8,
    /// Associated data.
    pub data: *mut u8,
}

/// Hash action: find existing entry only.
pub const FIND: i32 = 0;
/// Hash action: enter (insert if not found).
pub const ENTER: i32 = 1;

/// Bucket node for separate chaining.
#[repr(C)]
struct HashNode {
    entry: Entry,
    next: *mut HashNode,
}

/// Global hash table state.
struct HashTable {
    buckets: *mut *mut HashNode,
    size: usize,
}

/// Global hash table (POSIX only defines one table at a time).
static mut HTAB: HashTable = HashTable {
    buckets: core::ptr::null_mut(),
    size: 0,
};

/// FNV-1a hash for NUL-terminated strings.
fn fnv1a_hash(key: *const u8) -> u64 {
    const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0100_0000_01b3;

    let mut h = FNV_OFFSET;
    if !key.is_null() {
        let mut p = key;
        // SAFETY: key is a valid NUL-terminated string per POSIX contract.
        unsafe {
            while *p != 0 {
                h ^= u64::from(*p);
                h = h.wrapping_mul(FNV_PRIME);
                p = p.add(1);
            }
        }
    }
    h
}

/// Compare two NUL-terminated C strings for equality.
///
/// Returns true if the strings are byte-for-byte equal.
fn c_str_eq(a: *const u8, b: *const u8) -> bool {
    if a.is_null() || b.is_null() {
        return false;
    }
    if a == b {
        return true;
    }
    // SAFETY: both pointers are valid NUL-terminated strings.
    unsafe {
        let mut pa = a;
        let mut pb = b;
        loop {
            if *pa != *pb {
                return false;
            }
            if *pa == 0 {
                return true;
            }
            pa = pa.add(1);
            pb = pb.add(1);
        }
    }
}

/// `hcreate` — create a hash table.
///
/// Creates a global hash table with at least `nel` entries capacity.
/// Returns non-zero on success, 0 on failure (sets errno).
///
/// POSIX: only one hash table may be active at a time.  Calling
/// `hcreate` when a table already exists is undefined behavior;
/// we silently destroy the old one.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn hcreate(nel: usize) -> i32 {
    // SAFETY: `HTAB` is the single process-global table the POSIX
    // `hsearch` family is defined around, and that family is explicitly
    // not thread-safe -- serialising calls is the caller's obligation
    // (`hsearch_r` is the reentrant form for callers who need one).  So
    // the sole accessor here is the calling thread.  This crate's own
    // tests are such a caller: see `HTAB_TEST_LOCK` in the test module,
    // added after unsynchronised tests segfaulted the test binary.
    // NOTE: this used to read "single-threaded access", which asserted a
    // fact rather than naming an obligation, and was false in the tests.
    unsafe {
        // Destroy any existing table.
        if !HTAB.buckets.is_null() {
            hdestroy();
        }

        // Allocate at least `nel` buckets (use next power of two for
        // good distribution, minimum 16).
        let mut size = 16_usize;
        while size < nel {
            size = if let Some(s) = size.checked_mul(2) {
                s
            } else {
                errno::set_errno(errno::ENOMEM);
                return 0;
            };
        }

        let Some(alloc_bytes) = size.checked_mul(core::mem::size_of::<*mut HashNode>()) else {
            errno::set_errno(errno::ENOMEM);
            return 0;
        };

        let ptr = crate::malloc::malloc(alloc_bytes);
        if ptr.is_null() {
            errno::set_errno(errno::ENOMEM);
            return 0;
        }

        // Zero all bucket pointers.
        core::ptr::write_bytes(ptr, 0, alloc_bytes);

        HTAB.buckets = ptr.cast::<*mut HashNode>();
        HTAB.size = size;
    }
    1 // success
}

/// `hdestroy` — destroy the global hash table.
///
/// Frees all bucket chains and the bucket array.  Does not free
/// the key or data pointers in each entry (POSIX does not require it).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn hdestroy() {
    // SAFETY: `HTAB` is the single process-global table the POSIX
    // `hsearch` family is defined around, and that family is explicitly
    // not thread-safe -- serialising calls is the caller's obligation
    // (`hsearch_r` is the reentrant form for callers who need one).  So
    // the sole accessor here is the calling thread.  This crate's own
    // tests are such a caller: see `HTAB_TEST_LOCK` in the test module,
    // added after unsynchronised tests segfaulted the test binary.
    // NOTE: this used to read "single-threaded access", which asserted a
    // fact rather than naming an obligation, and was false in the tests.
    unsafe {
        if HTAB.buckets.is_null() {
            return;
        }

        // Free all chains.
        let mut i: usize = 0;
        while i < HTAB.size {
            let mut node = *HTAB.buckets.add(i);
            while !node.is_null() {
                let next = (*node).next;
                crate::malloc::free(node.cast::<u8>());
                node = next;
            }
            i = i.wrapping_add(1);
        }

        crate::malloc::free(HTAB.buckets.cast::<u8>());
        HTAB.buckets = core::ptr::null_mut();
        HTAB.size = 0;
    }
}

/// `hsearch` — search or enter an item in the hash table.
///
/// If `action` is `FIND`, searches for the entry with key `item.key`.
/// If `action` is `ENTER`, inserts the entry if not found.
///
/// Returns a pointer to the matching `Entry`, or null if not found
/// (FIND) or allocation failed (ENTER).  Sets errno to ESRCH on
/// not-found, ENOMEM on allocation failure.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn hsearch(item: Entry, action: i32) -> *mut Entry {
    // SAFETY: `HTAB` is the single process-global table the POSIX
    // `hsearch` family is defined around, and that family is explicitly
    // not thread-safe -- serialising calls is the caller's obligation
    // (`hsearch_r` is the reentrant form for callers who need one).  So
    // the sole accessor here is the calling thread.  This crate's own
    // tests are such a caller: see `HTAB_TEST_LOCK` in the test module,
    // added after unsynchronised tests segfaulted the test binary.
    // NOTE: this used to read "single-threaded access", which asserted a
    // fact rather than naming an obligation, and was false in the tests.
    unsafe {
        if HTAB.buckets.is_null() || HTAB.size == 0 {
            errno::set_errno(errno::ESRCH);
            return core::ptr::null_mut();
        }

        let hash = fnv1a_hash(item.key);
        let idx = (hash as usize) & (HTAB.size.wrapping_sub(1));
        let bucket = HTAB.buckets.add(idx);

        // Search the chain.
        let mut node = *bucket;
        while !node.is_null() {
            if c_str_eq((*node).entry.key, item.key) {
                return &raw mut (*node).entry;
            }
            node = (*node).next;
        }

        // Not found.
        if action == FIND {
            errno::set_errno(errno::ESRCH);
            return core::ptr::null_mut();
        }

        // ENTER: allocate a new node and prepend to bucket.
        let new_node = crate::malloc::malloc(core::mem::size_of::<HashNode>());
        if new_node.is_null() {
            errno::set_errno(errno::ENOMEM);
            return core::ptr::null_mut();
        }
        let new_node = new_node.cast::<HashNode>();
        (*new_node).entry.key = item.key;
        (*new_node).entry.data = item.data;
        (*new_node).next = *bucket;
        *bucket = new_node;

        &raw mut (*new_node).entry
    }
}

// ===========================================================================
// Linear search (lsearch / lfind)
// ===========================================================================

/// Linear search comparison function type.
pub type LsearchComparFn = extern "C" fn(*const u8, *const u8) -> i32;

/// `lfind` — linear search without insertion.
///
/// Searches the array `base` of `*nelp` elements, each of `width`
/// bytes, for a member matching `key` using `compar`.
///
/// Returns a pointer to the matching element, or null if not found.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lfind(
    key: *const u8,
    base: *const u8,
    nelp: *const usize,
    width: usize,
    compar: LsearchComparFn,
) -> *const u8 {
    if key.is_null() || base.is_null() || nelp.is_null() || width == 0 {
        return core::ptr::null();
    }

    // SAFETY: nelp is valid per caller's contract.
    let n = unsafe { *nelp };
    let mut i: usize = 0;
    while i < n {
        // SAFETY: base + i*width is within the array.
        let elem = unsafe { base.add(i.wrapping_mul(width)) };
        if compar(key, elem) == 0 {
            return elem;
        }
        i = i.wrapping_add(1);
    }
    core::ptr::null()
}

/// `lsearch` — linear search with insertion.
///
/// Like `lfind`, but if the key is not found, appends it to the array
/// (copies `width` bytes from `key` to the end) and increments `*nelp`.
///
/// Returns a pointer to the matching or newly-inserted element.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn lsearch(
    key: *const u8,
    base: *mut u8,
    nelp: *mut usize,
    width: usize,
    compar: LsearchComparFn,
) -> *mut u8 {
    if key.is_null() || base.is_null() || nelp.is_null() || width == 0 {
        return core::ptr::null_mut();
    }

    // Search first.
    let found = lfind(key, base, nelp, width, compar);
    if !found.is_null() {
        return found.cast_mut();
    }

    // Not found — append.
    // SAFETY: nelp is valid, and caller guarantees the array has room.
    unsafe {
        let n = *nelp;
        let dest = base.add(n.wrapping_mul(width));
        core::ptr::copy_nonoverlapping(key, dest, width);
        *nelp = n.wrapping_add(1);
        dest
    }
}

// ===========================================================================
// Linked list (insque / remque)
// ===========================================================================

/// Doubly-linked list element layout for `insque`/`remque`.
///
/// The first two fields of the user's struct must be forward and
/// backward pointers (like POSIX requires).
#[repr(C)]
struct QueueEntry {
    /// Forward pointer (next element).
    next: *mut QueueEntry,
    /// Backward pointer (previous element).
    prev: *mut QueueEntry,
    // User data follows...
}

/// `insque` — insert an element into a doubly-linked list.
///
/// Inserts `elem` after `pred`.  If `pred` is null, `elem` becomes
/// the sole element (head of a new list).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn insque(elem: *mut u8, pred: *mut u8) {
    if elem.is_null() {
        return;
    }

    let e = elem.cast::<QueueEntry>();

    if pred.is_null() {
        // Start a new list — elem points to itself / null.
        // SAFETY: elem is valid per caller.
        unsafe {
            (*e).next = core::ptr::null_mut();
            (*e).prev = core::ptr::null_mut();
        }
        return;
    }

    let p = pred.cast::<QueueEntry>();
    // SAFETY: pred and elem are valid per caller.
    unsafe {
        let after = (*p).next;
        (*e).next = after;
        (*e).prev = p;
        (*p).next = e;
        if !after.is_null() {
            (*after).prev = e;
        }
    }
}

/// `remque` — remove an element from a doubly-linked list.
///
/// Unlinks `elem` from the list by patching the forward and backward
/// pointers of its neighbors.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn remque(elem: *mut u8) {
    if elem.is_null() {
        return;
    }

    let e = elem.cast::<QueueEntry>();
    // SAFETY: elem is valid per caller.
    unsafe {
        let prev = (*e).prev;
        let next = (*e).next;
        if !prev.is_null() {
            (*prev).next = next;
        }
        if !next.is_null() {
            (*next).prev = prev;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    /// Serialises every test that touches the process-global `HTAB`.
    ///
    /// `hcreate`/`hsearch`/`hdestroy` implement the POSIX single-table API, so
    /// there is exactly one table per process and no way to give each test its
    /// own — unlike the walker counters below, which simply stop being shared.
    /// The only thing left to fix is therefore the concurrency, and it has to
    /// be fixed: `hdestroy` frees `HTAB.buckets` and nulls it, while `hsearch`
    /// null-checks it and *then* dereferences it, and those are not one atomic
    /// step. A test that reaches the dereference just as another test's
    /// `hdestroy` frees the array reads freed memory, which is why this showed
    /// up as `STATUS_ACCESS_VIOLATION` killing the whole binary rather than as
    /// a failing assertion — a crash costs all 20 500 results in the process,
    /// not one.
    ///
    /// Hold it for the *whole* test body, from before `hcreate` to past
    /// `hdestroy`, so no test can observe a half-built or half-freed table.
    static HTAB_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Acquire the `HTAB` lock, recovering from poison.
    ///
    /// Poison recovery matters here: without it, the first test to fail an
    /// assertion while holding the lock would turn six sibling failures into
    /// the report, burying the one real cause.
    #[must_use = "the guard serialises HTAB-touching tests; bind it to `_g`"]
    fn lock_htab_for_test() -> std::sync::MutexGuard<'static, ()> {
        HTAB_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Integer comparison function for tests.
    extern "C" fn int_compar(a: *const u8, b: *const u8) -> i32 {
        let va = a as i64;
        let vb = b as i64;
        if va < vb {
            -1
        } else if va > vb {
            1
        } else {
            0
        }
    }

    // -- Action constants --

    #[test]
    fn test_action_constants() {
        assert_eq!(PREORDER, 0);
        assert_eq!(POSTORDER, 1);
        assert_eq!(ENDORDER, 2);
        assert_eq!(LEAF, 3);
    }

    // -- tsearch / tfind --

    #[test]
    fn test_tsearch_null_rootp() {
        let ret = tsearch(1 as *const u8, core::ptr::null_mut(), int_compar);
        assert!(ret.is_null());
    }

    #[test]
    fn test_tfind_null_rootp() {
        let ret = tfind(1 as *const u8, core::ptr::null(), int_compar);
        assert!(ret.is_null());
    }

    #[test]
    fn test_tsearch_insert_one() {
        let mut root: *mut u8 = core::ptr::null_mut();
        let ret = tsearch(42 as *const u8, &raw mut root, int_compar);
        // malloc may fail on test host — skip if so.
        if ret.is_null() {
            return;
        }
        assert!(!root.is_null(), "root should be set after insert");

        // Find it.
        let found = tfind(42 as *const u8, &raw const root, int_compar);
        assert!(!found.is_null(), "should find inserted key");

        // Don't find a different key.
        let not_found = tfind(99 as *const u8, &raw const root, int_compar);
        assert!(not_found.is_null(), "should not find non-existent key");

        tdestroy(root, Some(dummy_free));
    }

    #[test]
    fn test_tsearch_insert_duplicate() {
        let mut root: *mut u8 = core::ptr::null_mut();
        let ret1 = tsearch(10 as *const u8, &raw mut root, int_compar);
        if ret1.is_null() {
            return;
        }
        let ret2 = tsearch(10 as *const u8, &raw mut root, int_compar);
        assert_eq!(ret1, ret2, "duplicate insert should return same node");

        tdestroy(root, Some(dummy_free));
    }

    #[test]
    fn test_tsearch_insert_multiple() {
        let mut root: *mut u8 = core::ptr::null_mut();
        for v in [50, 25, 75, 10, 30, 60, 90] {
            let ret = tsearch(v as *const u8, &raw mut root, int_compar);
            if ret.is_null() {
                // malloc failed — clean up and skip.
                tdestroy(root, Some(dummy_free));
                return;
            }
        }

        // All should be findable.
        for v in [50, 25, 75, 10, 30, 60, 90] {
            let found = tfind(v as *const u8, &raw const root, int_compar);
            assert!(!found.is_null(), "should find key {v}");
        }

        let nf = tfind(42 as *const u8, &raw const root, int_compar);
        assert!(nf.is_null());

        tdestroy(root, Some(dummy_free));
    }

    // -- tdelete --

    #[test]
    fn test_tdelete_null_rootp() {
        let ret = tdelete(1 as *const u8, core::ptr::null_mut(), int_compar);
        assert!(ret.is_null());
    }

    #[test]
    fn test_tdelete_not_found() {
        let mut root: *mut u8 = core::ptr::null_mut();
        let ret = tsearch(10 as *const u8, &raw mut root, int_compar);
        if ret.is_null() {
            return;
        }
        let ret = tdelete(99 as *const u8, &raw mut root, int_compar);
        assert!(
            ret.is_null(),
            "deleting non-existent key should return null"
        );

        tdestroy(root, Some(dummy_free));
    }

    #[test]
    fn test_tdelete_leaf() {
        let mut root: *mut u8 = core::ptr::null_mut();
        for v in [50, 25, 75] {
            if tsearch(v as *const u8, &raw mut root, int_compar).is_null() {
                tdestroy(root, Some(dummy_free));
                return;
            }
        }

        let ret = tdelete(25 as *const u8, &raw mut root, int_compar);
        assert!(!ret.is_null());
        assert!(tfind(25 as *const u8, &raw const root, int_compar).is_null());
        assert!(!tfind(50 as *const u8, &raw const root, int_compar).is_null());
        assert!(!tfind(75 as *const u8, &raw const root, int_compar).is_null());

        tdestroy(root, Some(dummy_free));
    }

    #[test]
    fn test_tdelete_root() {
        let mut root: *mut u8 = core::ptr::null_mut();
        if tsearch(50 as *const u8, &raw mut root, int_compar).is_null() {
            return;
        }

        let ret = tdelete(50 as *const u8, &raw mut root, int_compar);
        assert!(
            root.is_null(),
            "root should be null after deleting only node"
        );
        // POSIX: an unspecified *non-null* pointer when the root goes.  It
        // was the new root -- null here -- until 2026-09-26, which a caller
        // testing for "not found" could not tell from failure.
        assert_eq!(ret, (&raw mut root).cast::<u8>());
    }

    // -- the red-black tree --

    /// Check the red-black invariants below `n` and return its black
    /// height: no red node has a red child, every path has the same number
    /// of black nodes, and the keys are in order.
    fn check_rb(n: *mut Node, lo: i64, hi: i64) -> usize {
        if n.is_null() {
            return 1;
        }
        let (key, left, right, red) = unsafe { ((*n).key as i64, (*n).left, (*n).right, (*n).red) };
        assert!(lo < key && key < hi, "key {key} out of order ({lo}, {hi})");
        if red {
            assert!(
                !unsafe { is_red(left) } && !unsafe { is_red(right) },
                "red {key} has a red child"
            );
        }
        let bl = check_rb(left, lo, key);
        let br = check_rb(right, key, hi);
        assert_eq!(bl, br, "black heights differ below {key}");
        bl + usize::from(!red)
    }

    fn height(n: *mut Node) -> usize {
        if n.is_null() {
            0
        } else {
            1 + height(unsafe { (*n).left }).max(height(unsafe { (*n).right }))
        }
    }

    fn check_tree(root: *mut u8) {
        let n = root.cast::<Node>();
        assert!(!unsafe { is_red(n) }, "the root is black");
        check_rb(n, i64::MIN, i64::MAX);
    }

    /// Keys in order made the old tree a list, n deep.  A red-black tree of
    /// n nodes is at most 2 * log2(n + 1) deep.
    #[test]
    fn test_tsearch_sorted_input_stays_balanced() {
        let mut root: *mut u8 = core::ptr::null_mut();
        let n = 20_000usize;
        for v in 1..=n {
            assert!(!tsearch(v as *const u8, &raw mut root, int_compar).is_null());
        }
        check_tree(root);
        let h = height(root.cast());
        assert!(
            h <= 2 * (usize::BITS - (n + 1).leading_zeros()) as usize,
            "height {h}"
        );
        for v in (1..=n).rev().step_by(7) {
            assert!(
                !tfind(v as *const u8, &raw const root, int_compar).is_null(),
                "{v}"
            );
        }
        tdestroy(root, None);
    }

    /// Inserts and deletes in a scrambled order, the invariants checked
    /// after every deletion, against a set that knows the answer.
    #[test]
    fn test_tdelete_keeps_the_tree_valid() {
        let mut root: *mut u8 = core::ptr::null_mut();
        let mut present = std::collections::BTreeSet::new();
        let mut x: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            (x % 1000) as i64 + 1
        };
        for _ in 0..3000 {
            let k = next();
            assert!(!tsearch(k as *const u8, &raw mut root, int_compar).is_null());
            present.insert(k);
        }
        check_tree(root);
        for _ in 0..3000 {
            let k = next();
            let ret = tdelete(k as *const u8, &raw mut root, int_compar);
            assert_eq!(!ret.is_null(), present.remove(&k), "delete {k}");
            if !root.is_null() {
                check_tree(root);
            }
        }
        for k in 1..=1000i64 {
            let found = !tfind(k as *const u8, &raw const root, int_compar).is_null();
            assert_eq!(found, present.contains(&k), "find {k}");
        }
        // Down to nothing, in order, and each root deletion still non-null.
        for k in present.clone() {
            assert!(!tdelete(k as *const u8, &raw mut root, int_compar).is_null());
            if !root.is_null() {
                check_tree(root);
            }
        }
        assert!(root.is_null());
    }

    /// `tdelete` answers with the deleted node's parent.
    #[test]
    fn test_tdelete_returns_the_parent() {
        let mut root: *mut u8 = core::ptr::null_mut();
        for v in [2, 1, 3] {
            assert!(!tsearch(v as *const u8, &raw mut root, int_compar).is_null());
        }
        // 2 is the root, 1 and 3 its children.
        let parent = tdelete(3 as *const u8, &raw mut root, int_compar);
        assert_eq!(parent, root, "3's parent is the root");
        assert_eq!(unsafe { (*parent.cast::<Node>()).key } as i64, 2);
        tdestroy(root, None);
    }

    std::thread_local! {
        /// `(key, visit, depth)` for each `twalk` callback on this thread.
        static VISITS: core::cell::RefCell<std::vec::Vec<(i64, i32, i32)>> =
            const { core::cell::RefCell::new(std::vec::Vec::new()) };
    }

    extern "C" fn record_visit(node: *const u8, visit: i32, depth: i32) {
        let key = unsafe { *node.cast::<*const u8>() } as i64;
        VISITS.with(|v| v.borrow_mut().push((key, visit, depth)));
    }

    extern "C" fn record_visit_r(node: *const u8, visit: i32, closure: *mut u8) {
        let key = unsafe { *node.cast::<*const u8>() } as i64;
        VISITS.with(|v| v.borrow_mut().push((key, visit, closure as i32)));
    }

    /// 1, 2, 3 in order: the root is 2, with 1 and 3 below it as leaves.
    #[test]
    fn test_twalk_visits_in_posix_order() {
        let mut root: *mut u8 = core::ptr::null_mut();
        for v in [1, 2, 3] {
            assert!(!tsearch(v as *const u8, &raw mut root, int_compar).is_null());
        }
        VISITS.with(|v| v.borrow_mut().clear());
        twalk(root, Some(record_visit));
        let want = [
            (2, PREORDER, 0),
            (1, LEAF, 1),
            (2, POSTORDER, 0),
            (3, LEAF, 1),
            (2, ENDORDER, 0),
        ];
        VISITS.with(|v| assert_eq!(v.borrow().as_slice(), want));
        // twalk_r passes its closure where twalk passes the depth.
        VISITS.with(|v| v.borrow_mut().clear());
        twalk_r(root, Some(record_visit_r), 77 as *mut u8);
        VISITS.with(|v| assert!(v.borrow().iter().all(|&(_, _, c)| c == 77)));
        VISITS.with(|v| assert_eq!(v.borrow().len(), 5));
        // A null action walks nothing, as in glibc.
        twalk(root, None);
        twalk_r(root, None, core::ptr::null_mut());
        tdestroy(root, None);
    }

    /// A null free function frees the nodes and no keys.
    #[test]
    fn test_tdestroy_with_no_free_function() {
        let mut root: *mut u8 = core::ptr::null_mut();
        for v in 1..=100usize {
            assert!(!tsearch(v as *const u8, &raw mut root, int_compar).is_null());
        }
        reset_destroy_count();
        tdestroy(root, None);
        assert_eq!(destroy_count(), 0);
    }

    // -- twalk --

    std::thread_local! {
        /// Nodes `count_walker` has seen **on this thread**.
        ///
        /// Thread-local, not a global atomic, and the distinction is the whole
        /// bug: `libtest` gives every test its own thread but runs many at
        /// once, so a global counter is reset to zero by *sibling* tests in
        /// the middle of this one's walk. The assertion then compares this
        /// test's expected node count against a number some other test zeroed
        /// — a failure with nothing wrong in the code under test.
        ///
        /// Per-thread, the counter is perturbed only by this test, so it needs
        /// no lock and the tests keep running concurrently. (Same reasoning,
        /// and the same shape, as `malloc::live_allocations`.)
        static WALK_COUNT: Cell<i32> = const { Cell::new(0) };
    }

    /// A walker callback has to be a bare `extern "C"` function pointer, which
    /// cannot capture — hence a thread-local rather than a closure over a
    /// local counter.
    extern "C" fn count_walker(_node: *const u8, action: i32, _depth: i32) {
        if action == LEAF || action == POSTORDER {
            WALK_COUNT.with(|c| c.set(c.get().wrapping_add(1)));
        }
    }

    /// Zero this thread's walk counter.
    fn reset_walk_count() {
        WALK_COUNT.with(|c| c.set(0));
    }

    /// This thread's walk counter.
    fn walk_count() -> i32 {
        WALK_COUNT.with(Cell::get)
    }

    #[test]
    fn test_twalk_empty() {
        reset_walk_count();
        twalk(core::ptr::null(), Some(count_walker));
        assert_eq!(walk_count(), 0);
    }

    #[test]
    fn test_twalk_single() {
        let mut root: *mut u8 = core::ptr::null_mut();
        if tsearch(42 as *const u8, &raw mut root, int_compar).is_null() {
            return;
        }

        reset_walk_count();
        twalk(root, Some(count_walker));
        assert_eq!(walk_count(), 1, "single node = 1 leaf");

        tdestroy(root, Some(dummy_free));
    }

    #[test]
    fn test_twalk_multiple() {
        let mut root: *mut u8 = core::ptr::null_mut();
        for v in [50, 25, 75] {
            if tsearch(v as *const u8, &raw mut root, int_compar).is_null() {
                tdestroy(root, Some(dummy_free));
                return;
            }
        }

        reset_walk_count();
        twalk(root, Some(count_walker));
        assert_eq!(walk_count(), 3);

        tdestroy(root, Some(dummy_free));
    }

    // -- tdestroy --

    std::thread_local! {
        /// Keys `count_destroyer` has freed **on this thread**.  Thread-local
        /// for the same reason as `WALK_COUNT` above.
        static DESTROY_COUNT: Cell<i32> = const { Cell::new(0) };
    }

    extern "C" fn count_destroyer(_key: *mut u8) {
        DESTROY_COUNT.with(|c| c.set(c.get().wrapping_add(1)));
    }

    /// Zero this thread's destroy counter.
    fn reset_destroy_count() {
        DESTROY_COUNT.with(|c| c.set(0));
    }

    /// This thread's destroy counter.
    fn destroy_count() -> i32 {
        DESTROY_COUNT.with(Cell::get)
    }

    extern "C" fn dummy_free(_key: *mut u8) {}

    #[test]
    fn test_tdestroy_empty() {
        reset_destroy_count();
        tdestroy(core::ptr::null_mut(), Some(count_destroyer));
        assert_eq!(destroy_count(), 0);
    }

    #[test]
    fn test_tdestroy_calls_free_fn() {
        let mut root: *mut u8 = core::ptr::null_mut();
        for v in [50, 25, 75, 10, 90] {
            if tsearch(v as *const u8, &raw mut root, int_compar).is_null() {
                tdestroy(root, Some(dummy_free));
                return;
            }
        }

        reset_destroy_count();
        tdestroy(root, Some(count_destroyer));
        assert_eq!(
            destroy_count(),
            5,
            "tdestroy should call free_fn for each node"
        );
    }

    // -- Node layout --

    #[test]
    fn test_node_layout() {
        // `key` first: `tsearch`'s result is read as a pointer to the key.
        assert_eq!(core::mem::offset_of!(Node, key), 0);
        // Three pointers and the colour, padded: 32 bytes on 64-bit.
        assert_eq!(core::mem::size_of::<Node>(), 32);
    }

    // ===================================================================
    // Hash table tests
    // ===================================================================

    #[test]
    fn test_hash_action_constants() {
        assert_eq!(FIND, 0);
        assert_eq!(ENTER, 1);
    }

    #[test]
    fn test_entry_layout() {
        // Entry: key(*mut u8) + data(*mut u8) = 16 bytes on 64-bit.
        assert_eq!(core::mem::size_of::<Entry>(), 16);
    }

    #[test]
    fn test_fnv1a_empty() {
        let empty = b"\0";
        let h = fnv1a_hash(empty.as_ptr());
        // FNV-1a offset basis (no bytes hashed).
        assert_eq!(h, 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn test_fnv1a_null() {
        let h = fnv1a_hash(core::ptr::null());
        assert_eq!(h, 0xcbf2_9ce4_8422_2325);
    }

    #[test]
    fn test_fnv1a_different_strings() {
        let a = b"hello\0";
        let b = b"world\0";
        let ha = fnv1a_hash(a.as_ptr());
        let hb = fnv1a_hash(b.as_ptr());
        assert_ne!(ha, hb, "different strings should hash differently");
    }

    #[test]
    fn test_fnv1a_same_string() {
        let s = b"test\0";
        let h1 = fnv1a_hash(s.as_ptr());
        let h2 = fnv1a_hash(s.as_ptr());
        assert_eq!(h1, h2, "same string should hash identically");
    }

    #[test]
    fn test_c_str_eq_same() {
        let a = b"hello\0";
        assert!(c_str_eq(a.as_ptr(), a.as_ptr()));
    }

    #[test]
    fn test_c_str_eq_equal() {
        let a = b"hello\0";
        let b = b"hello\0";
        assert!(c_str_eq(a.as_ptr(), b.as_ptr()));
    }

    #[test]
    fn test_c_str_eq_different() {
        let a = b"hello\0";
        let b = b"world\0";
        assert!(!c_str_eq(a.as_ptr(), b.as_ptr()));
    }

    #[test]
    fn test_c_str_eq_null() {
        let a = b"hello\0";
        assert!(!c_str_eq(a.as_ptr(), core::ptr::null()));
        assert!(!c_str_eq(core::ptr::null(), a.as_ptr()));
    }

    #[test]
    fn test_c_str_eq_both_null() {
        assert!(!c_str_eq(core::ptr::null(), core::ptr::null()));
    }

    #[test]
    fn test_hcreate_basic() {
        let _g = lock_htab_for_test();
        let ret = hcreate(10);
        // malloc may fail — skip if so.
        if ret == 0 {
            return;
        }
        assert_eq!(ret, 1);
        hdestroy();
    }

    #[test]
    fn test_hdestroy_no_table() {
        let _g = lock_htab_for_test();
        // Calling hdestroy with no table should be safe.
        hdestroy();
    }

    #[test]
    fn test_hsearch_no_table() {
        let _g = lock_htab_for_test();
        // Make sure no table exists.
        hdestroy();
        let item = Entry {
            key: b"key\0".as_ptr() as *mut u8,
            data: core::ptr::null_mut(),
        };
        let ret = hsearch(item, FIND);
        assert!(ret.is_null());
    }

    #[test]
    fn test_hsearch_enter_and_find() {
        let _g = lock_htab_for_test();
        hdestroy(); // ensure clean state
        if hcreate(32) == 0 {
            return;
        } // malloc fail

        let key = b"mykey\0";
        let data = 42usize as *mut u8;
        let item = Entry {
            key: key.as_ptr() as *mut u8,
            data,
        };

        // Enter the item.
        let entered = hsearch(item, ENTER);
        if entered.is_null() {
            hdestroy();
            return; // malloc fail
        }

        // Find it back.
        let find_item = Entry {
            key: key.as_ptr() as *mut u8,
            data: core::ptr::null_mut(),
        };
        let found = hsearch(find_item, FIND);
        assert!(!found.is_null(), "should find entered item");
        assert_eq!(unsafe { (*found).data }, data);

        hdestroy();
    }

    #[test]
    fn test_hsearch_find_nonexistent() {
        let _g = lock_htab_for_test();
        hdestroy();
        if hcreate(32) == 0 {
            return;
        }

        let item = Entry {
            key: b"nosuchkey\0".as_ptr() as *mut u8,
            data: core::ptr::null_mut(),
        };
        let found = hsearch(item, FIND);
        assert!(found.is_null());

        hdestroy();
    }

    #[test]
    fn test_hsearch_enter_multiple() {
        let _g = lock_htab_for_test();
        hdestroy();
        if hcreate(64) == 0 {
            return;
        }

        let keys: [&[u8]; 4] = [b"alpha\0", b"beta\0", b"gamma\0", b"delta\0"];
        for (i, k) in keys.iter().enumerate() {
            let item = Entry {
                key: k.as_ptr() as *mut u8,
                data: i as *mut u8,
            };
            let r = hsearch(item, ENTER);
            if r.is_null() {
                hdestroy();
                return; // malloc fail
            }
        }

        // Verify all findable.
        for (i, k) in keys.iter().enumerate() {
            let item = Entry {
                key: k.as_ptr() as *mut u8,
                data: core::ptr::null_mut(),
            };
            let f = hsearch(item, FIND);
            assert!(
                !f.is_null(),
                "should find key {:?}",
                core::str::from_utf8(&k[..k.len() - 1])
            );
            assert_eq!(unsafe { (*f).data }, i as *mut u8);
        }

        hdestroy();
    }

    #[test]
    fn test_hsearch_enter_duplicate_returns_existing() {
        let _g = lock_htab_for_test();
        hdestroy();
        if hcreate(32) == 0 {
            return;
        }

        let key = b"dupkey\0";
        let item1 = Entry {
            key: key.as_ptr() as *mut u8,
            data: 1 as *mut u8,
        };
        let e1 = hsearch(item1, ENTER);
        if e1.is_null() {
            hdestroy();
            return;
        }

        // Enter again with different data — should return existing.
        let item2 = Entry {
            key: key.as_ptr() as *mut u8,
            data: 2 as *mut u8,
        };
        let e2 = hsearch(item2, ENTER);
        assert!(!e2.is_null());
        // POSIX: ENTER with existing key returns existing entry (data unchanged).
        assert_eq!(unsafe { (*e2).data }, 1 as *mut u8);

        hdestroy();
    }

    // ===================================================================
    // Linear search tests
    // ===================================================================

    extern "C" fn i32_compar(a: *const u8, b: *const u8) -> i32 {
        let va = a.cast::<i32>();
        let vb = b.cast::<i32>();
        let a_val = unsafe { *va };
        let b_val = unsafe { *vb };
        a_val.wrapping_sub(b_val)
    }

    #[test]
    fn test_lfind_found() {
        let arr: [i32; 5] = [10, 20, 30, 40, 50];
        let key: i32 = 30;
        let nel: usize = 5;
        let width = core::mem::size_of::<i32>();

        let result = lfind(
            (&raw const key).cast::<u8>(),
            arr.as_ptr().cast::<u8>(),
            &raw const nel,
            width,
            i32_compar,
        );
        assert!(!result.is_null());
        assert_eq!(unsafe { *(result.cast::<i32>()) }, 30);
    }

    #[test]
    fn test_lfind_not_found() {
        let arr: [i32; 5] = [10, 20, 30, 40, 50];
        let key: i32 = 99;
        let nel: usize = 5;
        let width = core::mem::size_of::<i32>();

        let result = lfind(
            (&raw const key).cast::<u8>(),
            arr.as_ptr().cast::<u8>(),
            &raw const nel,
            width,
            i32_compar,
        );
        assert!(result.is_null());
    }

    #[test]
    fn test_lfind_empty_array() {
        let key: i32 = 1;
        let nel: usize = 0;
        let width = core::mem::size_of::<i32>();

        let result = lfind(
            (&raw const key).cast::<u8>(),
            core::ptr::null(),
            &raw const nel,
            width,
            i32_compar,
        );
        assert!(result.is_null());
    }

    #[test]
    fn test_lfind_null_key() {
        let arr: [i32; 3] = [1, 2, 3];
        let nel: usize = 3;
        let result = lfind(
            core::ptr::null(),
            arr.as_ptr().cast::<u8>(),
            &raw const nel,
            4,
            i32_compar,
        );
        assert!(result.is_null());
    }

    #[test]
    fn test_lfind_first_element() {
        let arr: [i32; 3] = [100, 200, 300];
        let key: i32 = 100;
        let nel: usize = 3;
        let width = core::mem::size_of::<i32>();

        let result = lfind(
            (&raw const key).cast::<u8>(),
            arr.as_ptr().cast::<u8>(),
            &raw const nel,
            width,
            i32_compar,
        );
        assert!(!result.is_null());
        // Should point to the first element.
        assert_eq!(result as usize, arr.as_ptr() as usize);
    }

    #[test]
    fn test_lfind_last_element() {
        let arr: [i32; 3] = [100, 200, 300];
        let key: i32 = 300;
        let nel: usize = 3;
        let width = core::mem::size_of::<i32>();

        let result = lfind(
            (&raw const key).cast::<u8>(),
            arr.as_ptr().cast::<u8>(),
            &raw const nel,
            width,
            i32_compar,
        );
        assert!(!result.is_null());
        assert_eq!(unsafe { *(result.cast::<i32>()) }, 300);
    }

    #[test]
    fn test_lsearch_found() {
        let mut arr: [i32; 8] = [10, 20, 30, 0, 0, 0, 0, 0];
        let key: i32 = 20;
        let mut nel: usize = 3;
        let width = core::mem::size_of::<i32>();

        let result = lsearch(
            (&raw const key).cast::<u8>(),
            arr.as_mut_ptr().cast::<u8>(),
            &raw mut nel,
            width,
            i32_compar,
        );
        assert!(!result.is_null());
        assert_eq!(nel, 3, "nel should not change when found");
        assert_eq!(unsafe { *(result.cast::<i32>()) }, 20);
    }

    #[test]
    fn test_lsearch_insert() {
        let mut arr: [i32; 8] = [10, 20, 30, 0, 0, 0, 0, 0];
        let key: i32 = 99;
        let mut nel: usize = 3;
        let width = core::mem::size_of::<i32>();

        let result = lsearch(
            (&raw const key).cast::<u8>(),
            arr.as_mut_ptr().cast::<u8>(),
            &raw mut nel,
            width,
            i32_compar,
        );
        assert!(!result.is_null());
        assert_eq!(nel, 4, "nel should increment on insert");
        assert_eq!(arr[3], 99, "inserted value should be at end");
    }

    #[test]
    fn test_lsearch_null_params() {
        let result = lsearch(
            core::ptr::null(),
            core::ptr::null_mut(),
            core::ptr::null_mut(),
            4,
            i32_compar,
        );
        assert!(result.is_null());
    }

    #[test]
    fn test_lsearch_zero_width() {
        let mut arr: [i32; 4] = [1, 2, 3, 0];
        let key: i32 = 1;
        let mut nel: usize = 3;
        let result = lsearch(
            (&raw const key).cast::<u8>(),
            arr.as_mut_ptr().cast::<u8>(),
            &raw mut nel,
            0,
            i32_compar,
        );
        assert!(result.is_null());
    }

    // ===================================================================
    // Linked list tests
    // ===================================================================

    /// Test struct matching the QueueEntry layout requirement.
    #[repr(C)]
    struct TestQueueItem {
        next: *mut TestQueueItem,
        prev: *mut TestQueueItem,
        value: i32,
    }

    impl TestQueueItem {
        fn new(value: i32) -> Self {
            Self {
                next: core::ptr::null_mut(),
                prev: core::ptr::null_mut(),
                value,
            }
        }
    }

    #[test]
    fn test_insque_null_elem() {
        // Should not crash.
        insque(core::ptr::null_mut(), core::ptr::null_mut());
    }

    #[test]
    fn test_remque_null() {
        // Should not crash.
        remque(core::ptr::null_mut());
    }

    #[test]
    fn test_insque_single_element() {
        let mut a = TestQueueItem::new(1);
        insque((&raw mut a).cast::<u8>(), core::ptr::null_mut());
        assert!(a.next.is_null());
        assert!(a.prev.is_null());
    }

    #[test]
    fn test_insque_two_elements() {
        let mut a = TestQueueItem::new(1);
        let mut b = TestQueueItem::new(2);

        insque((&raw mut a).cast::<u8>(), core::ptr::null_mut());
        insque((&raw mut b).cast::<u8>(), (&raw mut a).cast::<u8>());

        // a -> b -> null
        assert_eq!(a.next, &raw mut b);
        assert!(a.prev.is_null());
        assert!(b.next.is_null());
        assert_eq!(b.prev, &raw mut a);
    }

    #[test]
    fn test_insque_three_elements() {
        let mut a = TestQueueItem::new(1);
        let mut b = TestQueueItem::new(2);
        let mut c = TestQueueItem::new(3);

        insque((&raw mut a).cast::<u8>(), core::ptr::null_mut());
        insque((&raw mut b).cast::<u8>(), (&raw mut a).cast::<u8>());
        // Insert c between a and b.
        insque((&raw mut c).cast::<u8>(), (&raw mut a).cast::<u8>());

        // a -> c -> b -> null
        assert_eq!(a.next, &raw mut c);
        assert_eq!(c.prev, &raw mut a);
        assert_eq!(c.next, &raw mut b);
        assert_eq!(b.prev, &raw mut c);
    }

    #[test]
    fn test_remque_middle() {
        let mut a = TestQueueItem::new(1);
        let mut b = TestQueueItem::new(2);
        let mut c = TestQueueItem::new(3);

        insque((&raw mut a).cast::<u8>(), core::ptr::null_mut());
        insque((&raw mut b).cast::<u8>(), (&raw mut a).cast::<u8>());
        insque((&raw mut c).cast::<u8>(), (&raw mut b).cast::<u8>());

        // a -> b -> c -> null
        // Remove b.
        remque((&raw mut b).cast::<u8>());

        // a -> c -> null
        assert_eq!(a.next, &raw mut c);
        assert_eq!(c.prev, &raw mut a);
    }

    #[test]
    fn test_remque_tail() {
        let mut a = TestQueueItem::new(1);
        let mut b = TestQueueItem::new(2);

        insque((&raw mut a).cast::<u8>(), core::ptr::null_mut());
        insque((&raw mut b).cast::<u8>(), (&raw mut a).cast::<u8>());

        remque((&raw mut b).cast::<u8>());

        assert!(a.next.is_null());
    }

    #[test]
    fn test_remque_head_with_successor() {
        let mut a = TestQueueItem::new(1);
        let mut b = TestQueueItem::new(2);

        insque((&raw mut a).cast::<u8>(), core::ptr::null_mut());
        insque((&raw mut b).cast::<u8>(), (&raw mut a).cast::<u8>());

        remque((&raw mut a).cast::<u8>());

        // b's prev should now be null.
        assert!(b.prev.is_null());
    }

    #[test]
    fn test_queue_entry_layout() {
        // QueueEntry: next + prev = 2 pointers = 16 bytes.
        assert_eq!(core::mem::size_of::<QueueEntry>(), 16);
    }
}
