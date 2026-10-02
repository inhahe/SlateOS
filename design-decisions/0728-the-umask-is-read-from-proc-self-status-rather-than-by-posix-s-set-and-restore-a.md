## 728. The umask is read from `/proc/self/status` rather than by POSIX's set-and-restore, and the read lives in one shared module

**Date:** 2026-08-30
**Lane:** B
**Decided by:** Claude (autonomous)

**In short:** Every program that creates files needs to know the *file-creation
mask* — the process-wide setting that says which permission bits new files may
not have. POSIX gives no way to read it: the only call, `umask(2)`, sets it and
hands back the old value, so reading means writing twice and living with a
moment in between where the mask is wrong. That moment was corrupting unrelated
tests. The mask is now read out of `/proc/self/status`, which is a read, and the
code that does it lives in one module instead of five copies.

### The problem this replaces

GNU's idiom, reproduced verbatim in five binaries here, is to set the mask to a
known value, keep what came back, and set it straight back again. In GNU that is
sound and this entry is not a criticism of it: `cp` is one thread that creates
nothing between the two calls, so nothing can observe the wrong mask. Under
`cargo test` it is not sound, because a binary's tests run on threads of one
process and the mask is per-process. `cp.rs`'s copy probed with `0777` — deny
everything — and, under `#[cfg(test)]`, probed on every single copy rather than
once. A file another test created inside that window arrived at mode `0000`. Two
of `cp`'s tests had been failing a few percent of runs for weeks, with a
`PermissionDenied` and a bare `assert!(ok)` that both pointed at the code under
test. See `known-issues.md`,
`B-READING-THE-UMASK-BY-SETTING-IT-CORRUPTED-OTHER-TESTS-MODES`.

### The options

**Leave it; serialise the readers with a mutex.** *What changes:* nothing — the
failures continue. A lock orders the threads that take it, and the thread being
harmed is calling `File::create` and takes nothing. This does not work, and
looking as though it might is the worst property it has.

**Leave it; run the tests single-threaded.** *What changes:* `cargo test` gets
several times slower, permanently, for every binary in the crate. It does fix
the race, at a cost paid by 86 binaries to serve a hazard in three.

**Keep the probe but probe with `umask(0)` instead of `umask(0777)`.** *What
changes:* the failures get rarer and change shape. The window remains; a file
created inside it is merely too permissive rather than unreadable, so the race
stops failing loudly and starts failing silently. Strictly worse as a *fix* —
which is why it is kept only as the fallback's behaviour.

**Read `/proc/self/status` (chosen).** *What changes:* the mask is never
written, so there is no window at all. Linux has published a `Umask:` line since
4.7, and `kernel/src/fs/procfs.rs` writes the same field on the target — so this
is not a host-only trick that the shipped binaries would lose.

### Why a shared module and not a fix in `cp.rs`

Because the copy in `cp.rs` was one of five, and the next binary that needed a
umask would have made six. The five had already drifted: two probed with `0`,
one with `0777`, one never restored the mask at all, and two of them were not
reading it in the first place — `mkdir` and `mkfifo` *clear* it deliberately,
because `-m` names an exact mode. A module makes that last distinction explicit
(they are documented as deliberately not migrating) rather than leaving it to be
rediscovered by whoever next greps for `umask`.

`userspace/chown` is a separate crate and cannot depend on `coreutils`, so it
keeps its own cached, permissive-direction probe. That is the same constraint
that moved `quote` out to `userspace/quoting` (370); if a third crate needs
this, the module should move out beside `quoting` rather than be copied again.

### The cost, and what is given up

The value is no longer cached inside the module, because a `/proc` read is cheap
and a live answer is the more useful one — `cp`'s tests set the mask around each
copy and must see the change. Callers that consult it per-file (`cp -r`) keep
their own `OnceLock`, so the caching decision is now the caller's, which is one
more thing for a caller to get wrong. Against that: the shared module cannot be
got wrong the way the five copies were, and the fallback path — a Unix with no
`/proc` — is the only place the old hazard survives.

### If it is ever unavailable

`from_proc` returning `None` falls through to the POSIX probe, so a kernel
without the field is slower to be correct, not incorrect. The probe uses
`umask(0)` on purpose: between two bad windows, one that permits too much and
one that denies everything, take the one that does not deny.

### Where

- `userspace/coreutils/src/umask.rs` — `current`, `from_proc`, `probe`, and the
  test that reads the mask a hundred times and checks it was not moved.
- `userspace/coreutils/src/lib.rs` — the eighteenth entry in the module
  narrative, including why `mkdir` and `mkfifo` are not in it.
- `cp.rs`'s `cached_umask`, `tar.rs`'s `read_umask`, `chmod.rs`'s `read_umask` —
  the three migrated callers.
