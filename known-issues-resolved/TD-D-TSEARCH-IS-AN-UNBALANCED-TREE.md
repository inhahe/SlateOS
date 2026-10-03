### [D] TD-D-TSEARCH-IS-AN-UNBALANCED-TREE — 2026-09-26 — FIXED 2026-09-26

**Fix.** glibc 2.39's misc/tsearch.c, ported: `tsearch` splits and rotates on
the way down, `tdelete` overwrites the key with its successor's, unchains the
successor and repairs a lost black node on the way up, and `twalk` visits in
the same order. `twalk_r` exists. Host tests check the red-black invariants
(no red node with a red child, one black height, keys in order) after every
deletion of a scrambled 3000-operation run, and a 20,000-key sorted build
stays within `2 * log2(n + 1)` of height. Two answers differ from before:
`tdelete` of the root now returns a non-null pointer (`rootp`) — it returned
the new root, null once the last node went, which read as "not found" — and a
null `twalk` action or `tdestroy` free function is accepted instead of being
undefined behaviour at the call.

**Where:** `posix/src/search.rs` — `tsearch`, `tfind`, `tdelete`, `twalk`,
`tdestroy`.

**In short:** the `<search.h>` binary tree is a plain, unbalanced binary
search tree. Keys inserted in order — the usual case: sorted input, increasing
ids, a file's lines — make it a linked list, so each insertion and lookup costs
O(n) and building a tree of n keys O(n²). glibc's is a red-black tree
(misc/tsearch.c), O(log n) each, and programs are written against that.

**Found by:** the ftw port (§1109), which needed a set of `(st_dev, st_ino)`
keys — inode numbers arrive in near-sorted order — and uses a private hash
table instead.

**Proper fix:** port glibc 2.39's misc/tsearch.c: the red-black insertion
with its top-down rebalancing, `tdelete`'s rebalancing, and `twalk`'s
preorder/postorder/endorder/leaf visits, which callers depend on the order of.
`twalk_r` (glibc 2.30) is missing too.
