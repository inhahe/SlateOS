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
    let ptr = crate::malloc::malloc(core::mem::size_of::<Node>());
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
                h ^= *p as u64;
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
    // SAFETY: single-threaded access to global state.
    unsafe {
        // Destroy any existing table.
        if !HTAB.buckets.is_null() {
            hdestroy();
        }

        // Allocate at least `nel` buckets (use next power of two for
        // good distribution, minimum 16).
        let mut size = 16_usize;
        while size < nel {
            size = match size.checked_mul(2) {
                Some(s) => s,
                None => {
                    errno::set_errno(errno::ENOMEM);
                    return 0;
                }
            };
        }

        let alloc_bytes = match size.checked_mul(core::mem::size_of::<*mut HashNode>()) {
            Some(b) => b,
            None => {
                errno::set_errno(errno::ENOMEM);
                return 0;
            }
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
    // SAFETY: single-threaded access.
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
    // SAFETY: single-threaded access to global table.
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
        return found as *mut u8;
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
        let ret = hcreate(10);
        // malloc may fail — skip if so.
        if ret == 0 { return; }
        assert_eq!(ret, 1);
        hdestroy();
    }

    #[test]
    fn test_hdestroy_no_table() {
        // Calling hdestroy with no table should be safe.
        hdestroy();
    }

    #[test]
    fn test_hsearch_no_table() {
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
        hdestroy(); // ensure clean state
        if hcreate(32) == 0 { return; } // malloc fail

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
        hdestroy();
        if hcreate(32) == 0 { return; }

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
        hdestroy();
        if hcreate(64) == 0 { return; }

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
            assert!(!f.is_null(), "should find key {:?}", core::str::from_utf8(&k[..k.len()-1]));
            assert_eq!(unsafe { (*f).data }, i as *mut u8);
        }

        hdestroy();
    }

    #[test]
    fn test_hsearch_enter_duplicate_returns_existing() {
        hdestroy();
        if hcreate(32) == 0 { return; }

        let key = b"dupkey\0";
        let item1 = Entry {
            key: key.as_ptr() as *mut u8,
            data: 1 as *mut u8,
        };
        let e1 = hsearch(item1, ENTER);
        if e1.is_null() { hdestroy(); return; }

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
