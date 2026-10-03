## B-AIO-TOOK-AN-EVENTFD-TO-NOTIFY-AND-NEVER-NOTIFIED-IT (lane B, 2026-09-13) — **fixed**, and so is its sibling

**In short:** a program can hand the kernel-AIO interface an eventfd and say
"poke this when my I/O finishes". We took the eventfd, ran the I/O, and never
poked it. A program that waits on that eventfd — which is the entire reason the
feature exists — waits forever.

### What it was

`posix/src/linux_aio_abi.rs` listed it under **Limitations**:

> `aio_resfd` / eventfd notification is silently ignored — the next
> `io_getevents` will see the completion regardless.

The second clause is true, and it is why the first looked survivable. It is
not. A caller sets `IOCB_FLAG_RESFD` *precisely so it does not have to poll
`io_getevents`* — typically because it is already in an epoll loop and wants
AIO completions to arrive there. Ignoring the flag does not degrade that caller
to polling; it hangs it, with no error anywhere. `aio_resfd` was a struct field
no code read.

Fixed: `io_submit` now increments the eventfd as it queues each completion, per
completion rather than per batch so the eventfd is never readable before the
event it announces exists. The decision is split into `completion_resfd()` so
it is testable — the eventfd is a kernel object that does nothing on the host
triple, but *whether* a notification is owed and *to which* descriptor is
arithmetic, and the defect was never asking the question. Five tests, including
that fd 0 is a real descriptor (treating zero as "unset" would silently drop
exactly one caller) and that an `aio_resfd` too large for an `i32` is refused
rather than cast into a plausible small fd belonging to someone else.

### The sibling, which was worse — fixed the same day

The same Limitations list has one more line:

> Per-I/O RWF_* flags (`aio_rw_flags`) are ignored.

`aio_rw_flags` is, like `aio_resfd` was, a field nothing reads. The flags it
carries are not hints:

| flag | what the caller asked for | what we do |
|---|---|---|
| `RWF_DSYNC` | the write is on stable storage before completion | report success without syncing |
| `RWF_SYNC` | as above, including metadata | report success without syncing |
| `RWF_NOWAIT` | fail with `EAGAIN` rather than block | block |
| `RWF_HIPRI` | poll for completion (a hint) | ignore — legitimately |

**`RWF_DSYNC` ignored is a durability lie**, and it is the kind that survives
until a power cut: a database or journal that asks for a synchronous write, is
told it succeeded, and finds the bytes absent after a crash. That is worse than
the eventfd hang, because the hang is at least visible while it is happening.

**Deferred one tick on purpose, then done.** It was written down rather than
patched immediately because the semantics needed deciding rather than typing,
and a durability bug fixed carelessly becomes a different durability bug. The
policy settled on asks one question per flag — *can we actually deliver this?*
— because the one answer never available is to accept a flag and not honour it:

| flag | answer | why |
|---|---|---|
| `RWF_HIPRI` | ignored | a scheduling hint with nothing observable behind it; the only one where ignoring is legitimate |
| `RWF_DSYNC` | honoured | `fdatasync` after a successful write |
| `RWF_SYNC` | honoured | `fsync`; wins when both are set, since it is the stronger promise |
| `RWF_NOWAIT` | `EAGAIN` | the flag means *fail rather than block*, and a synchronous executor always blocks, so failing IS the honest answer |
| `RWF_APPEND` | `EINVAL` | it makes `aio_offset` irrelevant and writes at end-of-file; we `pwrite` at the caller's offset, so ignoring it misplaces the bytes |
| unknown bits | `EINVAL` | as Linux does, and checked **first**, so a caller hears about the bit it got wrong rather than a consequence of it |

Two details that are the actual substance. Flags are decided **before** the I/O
runs — one we cannot honour has to stop the operation, not be discovered after
the bytes have moved. And if the post-write sync itself fails, its error
**replaces** the byte count: the caller asked for stable storage and did not get
it, so reporting how many bytes were written would reinstate precisely the lie
being removed.

The policy lives in a pure `plan_rw_flags()`, so the part that can be wrong is
the part a host test can reach. Reverting it to ignore-everything fails exactly
the four tests that assert a flag is honoured or refused.

### The pattern, stated once

This is the third instance today of *the library said yes and did nothing*:
argv over 64 KiB silently discarded, argv past 512 entries silently truncated,
and this. In every case the code was correct about the mechanism and honest in
its comment — `"fall back to no args"`, `"silently ignored"` — and in every
case the comment described a **defect** in the tone of a **design note**. The
tell is a Limitations list whose entries are phrased as things the caller will
not get, when what they actually describe is something the caller *asked for
and was told it received*.
