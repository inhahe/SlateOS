## B-EXPLORER-THE-DROP-ONTO-MY-OWN-SUBFOLDER-CHECK-COULD-BE-WALKED-AROUND (lane C, 2026-08-16) — FIXED

**In short:** dragging a folder onto a folder inside itself is refused, because
doing it would move a directory into its own descendant and lose the whole
subtree. The refusal compared the two paths as *text*, so writing the same
destination a different way — `photos/2024/../2024/summer`, or via a symlink —
slipped past it. Fixed by resolving both paths on the filesystem before
comparing.

### The bug

`check_nested_drop` in `apps/explorer/src/dropzone.rs`:

```rust
for src in sources {
    if src == target_dir { … }
    if target_dir.starts_with(src) { … }
}
```

`Path::starts_with` is a component-wise *textual* comparison. It does not
resolve `..`, and it does not know what a symlink points at. So
`check_nested_drop(&["/a/b"], "/a/b/c/../c")` is refused (the prefix is still
literally there) but `check_nested_drop(&["/a/b"], "/a/x/../b/c")` is not — the
literal prefix is `/a/x`, while the real target is `/a/b/c`, inside the source.

### The fix

Both sides are canonicalized before comparison, falling back to the literal path
when the filesystem cannot answer (a target that does not exist yet, a
permission error) — a check that refuses to run is worse than one that runs on
the text:

```rust
let real_target = canonical_or_literal(target_dir);
for src in sources {
    let real_src = canonical_or_literal(src);
    …
}
```

Error messages still quote the path the *user* typed or dragged, not its
canonical form; being told about `/mnt/data/photos` when you dragged
`~/photos` is a worse message even though it is a truer path.

### Status and scope

`apps/explorer/src/dropzone.rs` is **dead code today** — the module carries
`#![allow(dead_code)]`, `main.rs` has only `mod dropzone;`, and nothing calls
`check_nested_drop`. It is the drag-and-drop plumbing waiting on the toolkit's
drop events. Fixed now rather than when it is wired up, because the wiring is
where attention will be on the event flow, not on a containment predicate that
already looks correct.

### Verification

`nested_drop_sees_through_a_parent_component`, in the same file: builds a real
directory tree, then aims a drop at the source's own child written with a `..`
detour, and asserts it is refused. It fails against the old textual comparison.
