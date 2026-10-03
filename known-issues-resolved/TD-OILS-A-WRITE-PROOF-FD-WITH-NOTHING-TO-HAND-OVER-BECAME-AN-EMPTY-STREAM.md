### TD-OILS-A-WRITE-PROOF-FD-WITH-NOTHING-TO-HAND-OVER-BECAME-AN-EMPTY-STREAM. `sh -c 'echo W' 1<somedir` exited 0 — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `child_read_only_out`'s fallback.

**What.** A read-only description with no OS object to duplicate — a directory,
a source already closed, a handle the host declines to duplicate — was handed to
a child as `Stdio::null()`. An empty stream *accepts* writes, so the child's
`echo` succeeded and it exited 0 where bash's fails with `EBADF` and exits 1.
A directory is the reachable shape of this: the host gives up no usable handle
for one, and `1<dd` is a perfectly ordinary redirect.

```text
$ sh -c 'echo W' 1<dd 2>/dev/null; echo "rc=$?"
bash: rc=1
osh : rc=0            # null() took the bytes

$ sh -c 'echo W' 1<dd 2>&1; echo "rc=$?"   # and a dup of it, likewise
bash: rc=1
osh : rc=0
```

The shell's *own* writes were already right (`write_to_write_fd` answers a
`WriteFd::ReadOnly` with `EBADF` from the access mode alone); it was only what
a child was given that was wrong. TD-OILS-AN-EXTERNAL-CHILD-NEVER-GOT-A-READ-
ONLY-FD-1-OR-2 below made these shapes reachable in the first place — before it,
such a child inherited the terminal and the question never arose.

**Fix (2026-08-06).** `child_read_only_out` returns a new `ReadOnlyOut`, which
can say `Closed` where `Stdio` cannot, and the callers route that into the
existing `ClosedStd` mechanism — the same one `cmd >&-` uses. A closed
descriptor is not literally what bash gives (bash's is open and merely
unwritable), but it fails the child's write with the same `EBADF` and the same
status, which is all such a descriptor is ever asked for. `ChildOut` gained a
matching `Closed` variant so a `2>&1` copying such a fd 1 carries the missing
write half across, as a dup must.

**Tests.** `tests/corpus/a-std-fd-bound-to-a-read-only-source.sh` gained a
directory section — builtin, builtin on fd 2, a dup, and five external shapes
(`1<dd`, `2<dd`, `3<dd 1>&3`, a persistent `exec 1<dd`, and `1<dd 2>&1`). All
five external probes exit 0 without the fix.
