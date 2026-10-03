## Seven of the eight boot self-tests behind `if fat_ok` do not need a FAT root (lane A)

**Status:** FIXED 2026-08-22 — all seven, one commit and one boot test each, as
planned below. `if fat_ok` now guards only `fat::self_test` and the cache flush
that pairs with its writes, which is the one thing it was ever right about.

Two of the seven paid for the whole exercise on their own:

- Un-gating `vfs::self_test` failed on its very first CI run with
  `AlreadyExists`, surfacing a real data-loss-class bug in memfs's `rename` that
  had been invisible for as long as the gate had been there. See *"memfs
  `rename` refused to replace an existing destination"* below.
- Un-gating `notify::self_test` failed on its very first CI run with
  `coalescing expected 1 event, got 2` — a deterministic bug *in the suite
  itself*, which had never run in CI even once and so had never been observed to
  fail. See *"`notify::self_test` asserted absolute event counts on a shared
  root watch"* below.

That is the pattern worth naming: a gate that stops a test from running does not
keep the test correct, it only keeps it *unobserved*. Both bugs were present the
whole time; un-gating is what made them events rather than latent facts.

A third, `journal::self_test`, showed the same hazard before it could bite. The
journal is a shared append-only log written by every real VFS mutation, and its
checks counted entries over it; the boot that un-gated it reported `Start cursor:
191`, i.e. 191 unrelated entries already recorded by the time the suite began. It
was hardened in the same commit rather than un-gated as-is — see *"`journal::
self_test` counted entries in a log the whole kernel writes to"* below. The
general lesson, which cost two boot tests to learn: **a suite written under a
gate tends to assume the quiet world the gate gave it.** When un-gating one,
check its assertions for that assumption instead of only checking that it passes
once.

**In short:** The kernel runs a batch of filesystem self-tests at boot, but only
if it managed to mount a real FAT disk as the main drive. The automated boot
test does not use a FAT disk — it boots with an in-memory main drive — so that
whole batch has been skipped on every automated run, silently. Almost none of
those tests actually care about FAT; they were simply put inside the same `if`.
The result is that eight test suites for the ext4 filesystem had never run in
the automated test even once, on a disk that was sitting right there, mounted
and writable, the entire time.

### Evidence

The boot test's serial log says both halves of it plainly:

```
[vfs] Mounted memfs filesystem at '/' (rw)      <- root is memfs, so fat_ok == false
[vfs] Mounted ext4 filesystem at '/mnt' (rw)    <- ...but ext4 IS mounted, rw
```

and `grep '\[ext4\] Phase 0'` over a full 43,450-line log returns nothing.

### What each entry in the block actually requires

`fat_ok` means "the **FAT root** mounted". Measured against that, only the
first row is correctly gated:

| Call | Actually depends on | Self-skips if absent? |
|---|---|---|
| `fs::fat::self_test` | the FAT root — **correctly gated** | — |
| `fs::ext4::self_test` | an ext4 mount (Phase 1); Phase 0 is pure | yes — "No ext4 filesystem mounted" |
| `ipc::io_ring::self_test_fh` | `/tmp`, which is memfs, mounted unconditionally | yes — "SKIPPED (no FS)" |
| `fs::cache::self_test` | block device `vda` | yes — "No device 'vda' — skipping" |
| `fs::vfs::self_test` | any mounts; adapts via `has_tmp` | yes — "No mounts — skipping" |
| `fs::trash::self_test` | a **writable root** (memfs is `rw`) | no — propagates with `?` |
| `fs::notify::self_test` | a writable root (`/ACCESS_PROBE.TXT`) | no — propagates with `?` |
| `fs::journal::self_test` | a writable root (`/_JOURNAL`) | mostly — `stat` is matched |

Four of the seven already contain exactly the right guard *inside themselves*,
which is the tell: `fat_ok` was never load-bearing for them, because each one
had already been written to decide for itself.

### Fixed so far

`fs::ext4::self_test()` moved to the unconditional block (`kernel/src/main.rs`),
which puts eight per-module suites — `ondisk`, `superblock`, `io`, `balloc`,
`journal`, `fsck`, `driver`, `vfs_impl` — into CI for the first time, along with
the integration phase against `/mnt`. Phase 1 writes a scratch file to the ext4
mount; that is safe in the boot test because `rootfs.ext4` is attached with
`snapshot=on`, and a `flush_all()` was added after it so the dirty blocks it
leaves are not stranded (the flush it used to sit in front of is inside the
`fat_ok` block and no longer runs after it).

### The proper fix for the rest

Move each remaining call out of the `fat_ok` block, **one commit at a time with
a boot test between**, so that a newly-surfaced failure is attributable to the
suite that surfaced it rather than to a batch of six. Order by risk: the
self-skipping ones (`io_ring`, `cache`, `vfs`) are unconditionally safe and can
go first; `trash`/`notify`/`journal` need a writable root, which memfs provides
on every current boot path but which nothing checks — if a read-only-root path
is ever added they will emit a `WARNING:` line rather than skip, and the honest
fix there is to give them the same internal self-skip the other four already
have, not to invent a new gate.

### Why this matters beyond the missing coverage

This is the harness-level form of a pattern already recorded in this file: the
tests that run are the ones somebody needed while debugging, and an entire tier
next to them sits behind a condition that was never re-examined. A gate that
names one thing (`fat_ok`) while guarding eight is not a gate, it is a place
things got appended to.
