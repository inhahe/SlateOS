## 625. `/proc/meminfo` publishes the Linux keys it can source honestly, and refuses the two that exist only to report a policy we do not enforce

**Date:** 2026-08-27
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** `/proc/meminfo` is the file every memory-reporting program on a
Unix reads. Ours printed real numbers under *our own* names while the programs
look up the *Linux* names, and a name a program cannot find comes back as zero.
Mostly that meant honest columns of zeroes — but for swap it meant every tool
reporting the swap device as **completely full, always**, because they all work
out swap-in-use by subtracting `SwapFree` from the total, and `SwapFree` was not
there. This entry records which Linux names we added, why `MemAvailable` is
deliberately a *low* guess rather than an accurate-looking one, and why two
requested keys (`CommitLimit`, `Committed_AS`) are refused rather than filled in.

Lane B filed the request
(`requests/b-a-proc-meminfo-omits-the-linux-keys-that-thirteen-tools-read.md`)
after surveying `userspace/`: thirteen crates parse this file by Linux key name.

### The rule this applies

An absent key and a key published as `0` are **identical to a tool** — both
parse as zero. So publishing a truthful zero buys nothing mechanical. It is
still worth doing, for two reasons: it makes the value *sourced* rather than
*inferred*, so a human reading the file can tell the difference between "we
measured none" and "we never looked"; and it creates the line that must be
updated the day the number stops being zero.

That reasoning does **not** extend to publishing a *guess*. A key whose value we
cannot source is worse than absent, because absent is a state procps already
knows how to substitute for, whereas a wrong number is one it trusts.

### What was added, and from where

| Key | Source | Real? |
|---|---|---|
| `SwapFree` | `swap_total - swap_used`, saturating | yes — this is the fix |
| `Buffers` | buffer-cache `entries_used` × `SECTOR_SIZE` | yes |
| `Cached` | page-cache `resident` × `FRAME_SIZE` | yes |
| `Shmem` | `ipc::stats` live gauge | yes |
| `SReclaimable` | `0` | yes — the true value; no slab cache is marked reclaimable and there is no shrinker |
| `MemAvailable` | `MemFree` | a deliberate underestimate; see below |

### Why `MemAvailable` is `MemFree` and not something cleverer

`MemAvailable` answers "how much could a new workload allocate without pushing
the machine into swap". Linux estimates it as free memory plus roughly half the
reclaimable caches. We publish plain `MemFree`, which is exactly what procps
substitutes when the key is missing. Two better-looking candidates were
considered and both fail:

- **Add the buffer cache.** It is not reclaimable *at all*. The entry pool is
  allocated once at init and evicting an entry returns a slot to a free-list,
  not a frame to the allocator — so those bytes are held regardless of what
  `entries_used` says. This is worth stating plainly because the request assumed
  otherwise, and the assumption is the natural one.
- **Add the file page cache.** It *is* reclaimable, but only for entries with no
  live mapper: `page_cache::shrink` skips any frame with refcount > 1. Counting
  those exactly means walking every entry and taking the frame-allocator lock
  once per entry, while holding the page-cache lock — an unbounded loop over a
  hot lock, in a file `top` re-reads once a second. Adding the cache total
  *unreduced* would overstate, and an "available" figure is the one number that
  must never err upward: a user who trusts it and allocates gets the swap storm
  it promised wouldn't happen.

Halving the cache the way Linux does would be a guess wearing a number's
clothes. Ours is wrong in the safe direction and obviously so.

**What would change this:** a live counter of unmapped page-cache entries,
maintained where refcounts already change, would make the exact figure O(1) and
this becomes `MemFree + that`. The comment at the call site says so.

### Why `CommitLimit` / `Committed_AS` are refused

The request asked for these as "the only place a user can see the commitment
policy working", which is a fair motivation. They are still refused, for two
independent reasons either of which would be sufficient.

**First, precedent that is already law in this file.** `gen_sys_vm` deliberately
does not expose `overcommit_ratio` or `overcommit_kbytes`, citing §1 — never
advertise an unhonored feature — because those knobs only parameterise Linux's
*strict commit accounting* (`overcommit_memory = 2`), which we do not perform.
`CommitLimit` is defined by that same accounting and by nothing else. Refusing
the knobs thirty lines above while publishing the limit they compute would be
incoherent.

**Second, and decisive: publishing them would recreate the very bug this change
fixes.** `free -v` prints `Committed_AS` and `CommitLimit` as a used/total pair.
We *could* source `Committed_AS` truthfully — a live sum of non-guard VMA bytes,
which does shrink on `munmap` — but we cannot source a limit, because there is
no limit being enforced. A real numerator over a zero denominator reads as a
machine committed past its capacity: a false alarm of exactly the shape, and
exactly the direction, as the `SwapFree` one. Absent, both read as zero and
`free -v` prints an empty row, which is the honest picture.

So: both or neither, and the limit cannot be honest, therefore neither.

### The alternative that was rejected

Lane B could have special-cased our file in each tool ("if `SwapFree` is absent
but `SwapUsed` is present, derive one from the other") — and did exactly that in
`free` as a stopgap (624), because shipping a `free` that lies about swap was
not acceptable while the request was open. As a general fix it is wrong twice
over: it is a band-aid replicated across thirteen crates, each separately
maintained, which is the accumulation shape `CLAUDE.md` says to stop and
redesign at; and it cannot work at all for `MemAvailable`, `Cached` or
`Committed_AS`, which no tool can derive from anything the file contains. The
single place the mismatch can be fixed once is the producer.

624's substitution keys on the `SwapFree` line being *absent*, so it stops
firing by itself now that the line exists. No lane-B change is needed.

### How to reverse

Delete the added `push_str` calls in `gen_meminfo` and the `/proc/meminfo` block
in `procfs::self_test`. Nothing reads the new keys from inside the kernel, and
consumers that split on `:` and look up by name are unaffected by lines
disappearing — they would simply return to reading zero, and `free` would return
to its 624 substitution.
