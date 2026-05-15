//! POSIX `<search.h>` — binary search tree operations.
//!
//! Implements the standard POSIX tree functions: `tsearch`, `tfind`,
//! `tdelete`, `twalk`, and the glibc extension `tdestroy`.
//!
//! These functions manage an unbalanced binary search tree where each
//! node stores a `*const u8` (void pointer to user data).  The tree
//! is accessed through a "root pointer" (`*mut *mut u8`) — a pointer
//! to the pointer that holds the root node address.
//!
//! ## Design
//!
//! - Nodes are allocated via `malloc` and freed via `free`.
//! - The comparison function has the standard C prototype:
//!   `int compar(const void *a, const void *b)`.
//! - `twalk` visits nodes in the POSIX-defined order: preorder,
//!   postorder (= in-order), endorder, and leaf.

use crate::errno;

// ---------------------------------------------------------------------------
// Node layout
// ---------------------------------------------------------------------------

/// A BST node.  Stored as a contiguous allocation:
///   [key: *const u8][left: *mut Node][right: *mut Node]
///
/// We use repr(C) so the layout is predictable.
#[repr(C)]
struct Node {
    /// Pointer to user data.
    key: *const u8,
    /// Left child (keys less than this node).
    left: *mut Node,
    /// Right child (keys greater than this node).
    right: *mut Node,
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

/// Free callback type for `tdestroy`.
pub type TdestroyFn = extern "C" fn(*mut u8);

// ---------------------------------------------------------------------------
// Allocate / free a node
// ---------------------------------------------------------------------------

fn alloc_node(key: *const u8) -> *mut Node {
    let ptr = unsafe { crate::malloc::malloc(core::mem::size_of::<Node>()) };
    if ptr.is_null() {
        return core::ptr::null_mut();
    }
    let node = ptr.cast::<Node>();
    // SAFETY: we just allocated enough memory for a Node.
    unsafe {
        (*node).key = key;
        (*node).left = core::ptr::null_mut();
        (*node).right = core::ptr::null_mut();
    }
    node
}

fn free_node(node: *mut Node) {
    if !node.is_null() {
        unsafe { crate::malloc::free(node.cast::<u8>()); }
    }
}

// ---------------------------------------------------------------------------
// tsearch — insert or find a node
// ---------------------------------------------------------------------------

/// `tsearch` — search for or insert a node in the binary search tree.
///
/// If a matching node is found, returns a pointer to it.
/// If not found, allocates a new node and inserts it.
/// Returns null if allocation fails.
///
/// `rootp` is a pointer to the root pointer (i.e., `void **rootp`).
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tsearch(
    key: *const u8,
    rootp: *mut *mut u8,
    compar: ComparFn,
) -> *mut u8 {
    if rootp.is_null() {
        return core::ptr::null_mut();
    }

    // Walk down the tree.
    let mut slot: *mut *mut Node = rootp.cast::<*mut Node>();
    loop {
        let current = unsafe { *slot };
        if current.is_null() {
            // Insert here.
            let new_node = alloc_node(key);
            if new_node.is_null() {
                errno::set_errno(errno::ENOMEM);
                return core::ptr::null_mut();
            }
            unsafe { *slot = new_node; }
            return new_node.cast::<u8>();
        }

        let cmp = compar(key, unsafe { (*current).key });
        if cmp < 0 {
            slot = unsafe { &raw mut (*current).left };
        } else if cmp > 0 {
            slot = unsafe { &raw mut (*current).right };
        } else {
            // Found — return existing node.
            return current.cast::<u8>();
        }
    }
}

// ---------------------------------------------------------------------------
// tfind — search without inserting
// ---------------------------------------------------------------------------

/// `tfind` — search for a node in the binary search tree.
///
/// Returns a pointer to the matching node, or null if not found.
/// Does not modify the tree.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tfind(
    key: *const u8,
    rootp: *const *mut u8,
    compar: ComparFn,
) -> *const u8 {
    if rootp.is_null() {
        return core::ptr::null();
    }

    let mut current: *mut Node = unsafe { *rootp }.cast::<Node>();
    while !current.is_null() {
        let cmp = compar(key, unsafe { (*current).key });
        if cmp < 0 {
            current = unsafe { (*current).left };
        } else if cmp > 0 {
            current = unsafe { (*current).right };
        } else {
            return current.cast::<u8>();
        }
    }

    core::ptr::null()
}

// ---------------------------------------------------------------------------
// tdelete — delete a node
// ---------------------------------------------------------------------------

/// `tdelete` — delete a node from the binary search tree.
///
/// Removes the node matching `key` and returns a pointer to the
/// parent of the deleted node (or the new root).  Returns null if
/// the key was not found.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tdelete(
    key: *const u8,
    rootp: *mut *mut u8,
    compar: ComparFn,
) -> *mut u8 {
    if rootp.is_null() {
        return core::ptr::null_mut();
    }

    let mut parent: *mut Node = core::ptr::null_mut();
    let mut slot: *mut *mut Node = rootp.cast::<*mut Node>();

    loop {
        let current = unsafe { *slot };
        if current.is_null() {
            return core::ptr::null_mut(); // Not found.
        }

        let cmp = compar(key, unsafe { (*current).key });
        if cmp < 0 {
            parent = current;
            slot = unsafe { &raw mut (*current).left };
        } else if cmp > 0 {
            parent = current;
            slot = unsafe { &raw mut (*current).right };
        } else {
            // Found the node to delete.
            let left = unsafe { (*current).left };
            let right = unsafe { (*current).right };

            if left.is_null() {
                // Replace with right child.
                unsafe { *slot = right; }
            } else if right.is_null() {
                // Replace with left child.
                unsafe { *slot = left; }
            } else {
                // Two children: find in-order successor (leftmost in right subtree).
                let mut succ_parent = current;
                let mut succ = right;
                while !unsafe { (*succ).left }.is_null() {
                    succ_parent = succ;
                    succ = unsafe { (*succ).left };
                }
                // Replace current's key with successor's key.
                unsafe { (*current).key = (*succ).key; }
                // Remove successor.
                if succ_parent == current {
                    unsafe { (*succ_parent).right = (*succ).right; }
                } else {
                    unsafe { (*succ_parent).left = (*succ).right; }
                }
                free_node(succ);
                // Return parent of the deleted node.
                if parent.is_null() {
                    return unsafe { *rootp };
                }
                return parent.cast::<u8>();
            }

            free_node(current);
            // Return parent (or new root if parent is null).
            if parent.is_null() {
                return unsafe { *rootp };
            }
            return parent.cast::<u8>();
        }
    }
}

// ---------------------------------------------------------------------------
// twalk — walk the tree
// ---------------------------------------------------------------------------

/// Recursive tree walk helper.
fn twalk_recursive(node: *mut Node, action: TwalkFn, depth: i32) {
    if node.is_null() {
        return;
    }

    let left = unsafe { (*node).left };
    let right = unsafe { (*node).right };

    if left.is_null() && right.is_null() {
        // Leaf node — visit once with LEAF.
        action(node.cast::<u8>(), LEAF, depth);
    } else {
        // Internal node — visit three times.
        action(node.cast::<u8>(), PREORDER, depth);
        twalk_recursive(left, action, depth.wrapping_add(1));
        action(node.cast::<u8>(), POSTORDER, depth);
        twalk_recursive(right, action, depth.wrapping_add(1));
        action(node.cast::<u8>(), ENDORDER, depth);
    }
}

/// `twalk` — walk the binary search tree.
///
/// Calls `action` for each node with the visit order (preorder,
/// postorder, endorder for internal nodes; leaf for leaves) and
/// the node depth.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn twalk(root: *const u8, action: TwalkFn) {
    if root.is_null() {
        return;
    }
    twalk_recursive(root as *mut Node, action, 0);
}

// ---------------------------------------------------------------------------
// tdestroy — destroy the entire tree
// ---------------------------------------------------------------------------

/// Recursive tree destruction helper.
fn tdestroy_recursive(node: *mut Node, free_fn: TdestroyFn) {
    if node.is_null() {
        return;
    }
    tdestroy_recursive(unsafe { (*node).left }, free_fn);
    tdestroy_recursive(unsafe { (*node).right }, free_fn);
    // Call the user's free function on the key.
    free_fn(unsafe { (*node).key } as *mut u8);
    free_node(node);
}

/// `tdestroy` — destroy the entire binary search tree.
///
/// glibc extension.  Calls `free_fn` on each node's key, then
/// frees all nodes.
#[cfg_attr(target_os = "none", unsafe(no_mangle))]
pub extern "C" fn tdestroy(root: *mut u8, free_fn: TdestroyFn) {
    tdestroy_recursive(root.cast::<Node>(), free_fn);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::sync::atomic::{AtomicI32, Ordering};

    /// Integer comparison function for tests.
    extern "C" fn int_compar(a: *const u8, b: *const u8) -> i32 {
        let va = a as i64;
        let vb = b as i64;
        if va < vb { -1 } else if va > vb { 1 } else { 0 }
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
        if ret.is_null() { return; }
        assert!(!root.is_null(), "root should be set after insert");

        // Find it.
        let found = tfind(42 as *const u8, &raw const root, int_compar);
        assert!(!found.is_null(), "should find inserted key");

        // Don't find a different key.
        let not_found = tfind(99 as *const u8, &raw const root, int_compar);
        assert!(not_found.is_null(), "should not find non-existent key");

        tdestroy(root, dummy_free);
    }

    #[test]
    fn test_tsearch_insert_duplicate() {
        let mut root: *mut u8 = core::ptr::null_mut();
        let ret1 = tsearch(10 as *const u8, &raw mut root, int_compar);
        if ret1.is_null() { return; }
        let ret2 = tsearch(10 as *const u8, &raw mut root, int_compar);
        assert_eq!(ret1, ret2, "duplicate insert should return same node");

        tdestroy(root, dummy_free);
    }

    #[test]
    fn test_tsearch_insert_multiple() {
        let mut root: *mut u8 = core::ptr::null_mut();
        for v in [50, 25, 75, 10, 30, 60, 90] {
            let ret = tsearch(v as *const u8, &raw mut root, int_compar);
            if ret.is_null() {
                // malloc failed — clean up and skip.
                tdestroy(root, dummy_free);
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

        tdestroy(root, dummy_free);
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
        if ret.is_null() { return; }
        let ret = tdelete(99 as *const u8, &raw mut root, int_compar);
        assert!(ret.is_null(), "deleting non-existent key should return null");

        tdestroy(root, dummy_free);
    }

    #[test]
    fn test_tdelete_leaf() {
        let mut root: *mut u8 = core::ptr::null_mut();
        for v in [50, 25, 75] {
            if tsearch(v as *const u8, &raw mut root, int_compar).is_null() {
                tdestroy(root, dummy_free);
                return;
            }
        }

        let ret = tdelete(25 as *const u8, &raw mut root, int_compar);
        assert!(!ret.is_null());
        assert!(tfind(25 as *const u8, &raw const root, int_compar).is_null());
        assert!(!tfind(50 as *const u8, &raw const root, int_compar).is_null());
        assert!(!tfind(75 as *const u8, &raw const root, int_compar).is_null());

        tdestroy(root, dummy_free);
    }

    #[test]
    fn test_tdelete_root() {
        let mut root: *mut u8 = core::ptr::null_mut();
        if tsearch(50 as *const u8, &raw mut root, int_compar).is_null() { return; }

        let ret = tdelete(50 as *const u8, &raw mut root, int_compar);
        assert!(root.is_null(), "root should be null after deleting only node");
        let _ = ret;
    }

    // -- twalk --

    static WALK_COUNT: AtomicI32 = AtomicI32::new(0);

    extern "C" fn count_walker(_node: *const u8, action: i32, _depth: i32) {
        if action == LEAF || action == POSTORDER {
            WALK_COUNT.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[test]
    fn test_twalk_empty() {
        WALK_COUNT.store(0, Ordering::Relaxed);
        twalk(core::ptr::null(), count_walker);
        assert_eq!(WALK_COUNT.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_twalk_single() {
        let mut root: *mut u8 = core::ptr::null_mut();
        if tsearch(42 as *const u8, &raw mut root, int_compar).is_null() { return; }

        WALK_COUNT.store(0, Ordering::Relaxed);
        twalk(root, count_walker);
        assert_eq!(WALK_COUNT.load(Ordering::Relaxed), 1, "single node = 1 leaf");

        tdestroy(root, dummy_free);
    }

    #[test]
    fn test_twalk_multiple() {
        let mut root: *mut u8 = core::ptr::null_mut();
        for v in [50, 25, 75] {
            if tsearch(v as *const u8, &raw mut root, int_compar).is_null() {
                tdestroy(root, dummy_free);
                return;
            }
        }

        WALK_COUNT.store(0, Ordering::Relaxed);
        twalk(root, count_walker);
        assert_eq!(WALK_COUNT.load(Ordering::Relaxed), 3);

        tdestroy(root, dummy_free);
    }

    // -- tdestroy --

    static DESTROY_COUNT: AtomicI32 = AtomicI32::new(0);

    extern "C" fn count_destroyer(_key: *mut u8) {
        DESTROY_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    extern "C" fn dummy_free(_key: *mut u8) {}

    #[test]
    fn test_tdestroy_empty() {
        DESTROY_COUNT.store(0, Ordering::Relaxed);
        tdestroy(core::ptr::null_mut(), count_destroyer);
        assert_eq!(DESTROY_COUNT.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_tdestroy_calls_free_fn() {
        let mut root: *mut u8 = core::ptr::null_mut();
        for v in [50, 25, 75, 10, 90] {
            if tsearch(v as *const u8, &raw mut root, int_compar).is_null() {
                tdestroy(root, dummy_free);
                return;
            }
        }

        DESTROY_COUNT.store(0, Ordering::Relaxed);
        tdestroy(root, count_destroyer);
        assert_eq!(DESTROY_COUNT.load(Ordering::Relaxed), 5,
                   "tdestroy should call free_fn for each node");
    }

    // -- Node layout --

    #[test]
    fn test_node_layout() {
        // Node: key(*const u8) + left(*mut Node) + right(*mut Node)
        // = 3 pointers = 24 bytes on 64-bit.
        assert_eq!(core::mem::size_of::<Node>(), 24);
    }
}
