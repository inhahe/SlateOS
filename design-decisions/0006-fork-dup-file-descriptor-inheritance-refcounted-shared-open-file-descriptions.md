## 6. fork() / dup() file-descriptor inheritance — refcounted shared open-file descriptions

**Date:** 2026-05-31 (fork) / 2026-06-01 (dup fix); recorded retroactively 2026-06-12

**Decided by:** Claude (autonomous) — an implementation choice (and POSIX
correctness fix) made while building fork fd inheritance.

**Context:**
On `fork()`, the child's userspace libc fd table is CoW-copied, so it
references the **same kernel handle ids** as the parent — the kernel cannot
rewrite that userspace table. POSIX also requires that a forked child (and a
`dup()`/`dup2()`/`F_DUPFD` descriptor) **share one open file description**:
same file offset, same status flags. The kernel's `fs::handle` originally did
*not* refcount `OpenFile` — each id was a distinct entry and `handle::dup`
allocated a **new** id with an **independent cursor** (that is `dup()`-of-a-
*new-description* semantics, which is wrong for both fork sharing and POSIX
`dup`).

**Decision — refcount the open-file description and share ids.**
- Added a refcount to `OpenFile` plus `fs::handle::dup_shared(id)` (bump
  refcount, return the **same** id) and a refcount-aware `close` (the
  underlying description is released only when the last referencing fd closes).
- **fork** bumps refcounts on the existing ids rather than allocating new ones,
  matching pipes/sockets/eventfd which already did same-id refcounted dup.
- **dup()/dup2()/F_DUPFD** for `HandleKind::File` no longer call `SYS_FS_DUP`;
  the userspace `posix` crate shares the source fd's kernel handle id at the
  fd-table level via `alloc_fd_with_flags`, exactly like Pipe/Console/socket
  kinds. `close()` gates `SYS_FS_CLOSE` behind `is_handle_referenced()`.
- The old kernel `handle::dup` (independent cursor) is **left unchanged** and
  still used by `spawn.rs` fd inheritance, where a genuinely separate
  description is wanted.

**Rationale:**
- This is the only model that yields correct POSIX shared-offset semantics
  given that the kernel can't rewrite the child's userspace fd table — both
  ends *must* point at one refcounted description.
- Folding File into the same shared-id path the other handle kinds already use
  removes a special case and a latent dup() correctness bug in one stroke.

**Alternatives considered:**
- *Allocate fresh handle ids for the child on fork* — rejected: impossible to
  apply correctly (the kernel can't edit the CoW-copied userspace fd table) and
  semantically wrong (would give the child an independent offset).
- *Keep `handle::dup`'s independent-cursor behavior for dup()/dup2()* —
  rejected: that is a pre-existing POSIX bug (dup'd fds must share the
  description); fixed by routing File dup through fd-table id sharing.

**Where it lives:**
- `kernel/src/fs/handle.rs`: `OpenFile` refcount, `dup_shared`, refcount-aware
  `close`, `is_handle_referenced`.
- `posix/src/file.rs` (dup/dup2), `posix/src/fcntl_ops.rs::dup_fd_from`
  (F_DUPFD): File shares the id via `alloc_fd_with_flags`.
- `kernel/src/proc/fork.rs`: fd inheritance bumps refcounts.
- `posix` `fdtable.rs` module doc: documents the shared-id model.

**How to reverse:**
- Revert dup/dup2/F_DUPFD to `SYS_FS_DUP` + restore independent-cursor
  `handle::dup` for File (reintroduces the POSIX dup bug — not advisable).
- Drop the `OpenFile` refcount and have fork allocate new ids (breaks shared
  offset — not advisable). This decision is effectively load-bearing for POSIX
  correctness; reversal is only sensible if the fd model is redesigned.
