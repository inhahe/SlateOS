### BUG-OILS-DEVFD-IS-A-DUP-NOT-A-REOPEN. `> /dev/stdout` duplicates fd 1 where a host with a real `/dev/fd` re-opens it, so the two descriptors share an offset instead of getting independent ones — 2026-07-28 — APPROXIMATION

**Symptom.** With fd 1 on a *regular file*:

```
$ bash -c 'exec 2>/dev/stdout; echo E >&2; echo O' > f; cat f
O
$ osh  -c 'exec 2>/dev/stdout; echo E >&2; echo O' > f; cat f
E
O
```

bash opened `/dev/stdout` (a real path on this host, and a symlink to
`/proc/self/fd/1` on Linux), which creates a *new* open file description
at offset 0 — so `echo O` on fd 1 overwrites the `E\n` fd 2 wrote. osh
duplicates fd 1, so the two share one offset and the writes append.

The difference is invisible unless fd 1 is a seekable file: on a pipe, a
terminal, or a capture — the overwhelmingly common cases, and every probe
in `tests/corpus/redirect-dev-fd.sh` — a re-open and a dup behave
identically, and osh matches bash exactly.

**Where.** `userspace/oils/src/interp.rs` — `special_redirect_fd` and its
two consumers, `Shell::resolve_special_redirect` (transient redirects) and
`Shell::persistent_special_dup` (the `exec` path).

**Why it is not simply fixed.** osh has no real fd table: fd 1 is an
`Out` binding or an `Arc<File>`, and "re-open whatever file this handle
points at, truncating" is not expressible over that model (nor portable —
the Windows dev host has no `/dev` at all). The dup is also what bash
itself does on hosts built without `HAVE_DEV_FD`, and what the bash manual
documents, so it is the behaviour osh implements deliberately rather than
an oversight. Revisit only if SlateOS grows a `/dev/fd` and something is
found to depend on the independent-offset flavour.
