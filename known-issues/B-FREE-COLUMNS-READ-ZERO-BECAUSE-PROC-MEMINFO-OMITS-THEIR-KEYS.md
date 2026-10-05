## `B-FREE-COLUMNS-READ-ZERO-BECAUSE-PROC-MEMINFO-OMITS-THEIR-KEYS` (lane B, 2026-08-27) -- **open**, missing kernel data

**In short:** `free` works on this OS, but several of its columns are stuck at
zero -- not because `free` is wrong, but because the kernel's `/proc/meminfo`
does not publish the numbers they come from. It emits five memory keys where
Linux emits about fifty. This is a lane A change and the request is already
filed. Recorded here so the zeros are not mistaken for a bug in `free`.

`kernel/src/fs/procfs.rs`'s `gen_meminfo` emits `MemTotal`, `MemFree`,
`MemUsed`, `SwapTotal` and `SwapUsed`, plus SlateOS-specific counters that no
Linux tool reads. What that means for each column on a SlateOS machine today:

| Column | Reads | Because |
|---|---|---|
| `total`, `free`, `used` | correct | `MemTotal`/`MemFree` are present, and `used = MemTotal - MemAvailable` degrades to `MemTotal - MemFree` when `MemAvailable` is absent -- which is upstream's own fallback, not a divergence. |
| `available` | = `free` | No `MemAvailable`. Understates what is reclaimable, but in the safe direction. |
| `shared` | **0** | No `Shmem`. |
| `buff/cache` (and `-w`'s `buffers` and `cache`) | **0** | No `Buffers`, `Cached` or `SReclaimable`. |
| `Low:`/`High:` under `-l` | Low = the whole of memory, High = 0 | No `LowTotal`/`HighTotal`. This is also what `free` prints on any 64-bit Linux, so it is arguably correct rather than missing. |
| `Comm:` under `-v` | **0 0 0** | No `CommitLimit`/`Committed_AS`. |
| `Swap:` | correct | Only because of divergence 1 in the entry above. Without it this row would read 100% used on every machine with swap. |

**Where it lives:** the producer is `kernel/src/fs/procfs.rs::gen_meminfo`
(lane A); the consumer is `userspace/coreutils/src/bin/free.rs`. The request is
`requests/b-a-proc-meminfo-omits-the-linux-keys-that-thirteen-tools-read.md`,
which asks for `SwapFree`, `MemAvailable`, `Buffers`, `Cached`, `SReclaimable`,
`Shmem`, `CommitLimit` and `Committed_AS` to be added **additively**, leaving
the existing SlateOS keys in place.

**What would change this:** lane A landing that request. `free` needs no change
when it does -- every one of these columns is already wired to its key and
falls back only because the key is missing. Thirteen tools read this file, so
the fix is worth more than `free` alone.

**Appended by lane A, 2026-08-27 -- the producer side has landed.** `gen_meminfo`
now publishes `SwapFree`, `MemAvailable`, `Buffers`, `Cached`, `Shmem` and
`SReclaimable`, additively. Against the table above: `shared` and `buff/cache`
now carry real numbers, and `Swap:` is correct *on its own* rather than only via
624's substitution -- which stops firing by itself, since it keys on the
`SwapFree` line being absent.

Two rows do not change, both deliberately:

- **`available` still equals `free`.** `MemAvailable` is published, but its value
  is `MemFree`. The two candidates for improving on it both fail: the block
  buffer cache is not reclaimable at all (its entry pool is allocated once at
  init; eviction returns a slot to a free-list, not a frame to the allocator),
  and the file page cache is reclaimable only for entries with no live mapper,
  which cannot be counted without an unbounded walk holding a hot lock. It is an
  underestimate on purpose. A live counter of unmapped page-cache entries would
  make the exact figure O(1) and is the trigger to revisit.
- **`Comm:` still reads `0 0 0`.** `CommitLimit`/`Committed_AS` were declined,
  not missed: they report Linux's strict commit accounting, which we do not
  perform, and `gen_sys_vm` already refuses the knobs that parameterise it for
  that reason (§1). Publishing `Committed_AS` without a limit would put a real
  numerator over a zero denominator, which `free -v` renders as a machine
  committed past its capacity -- the same false alarm, in the same direction, as
  the `SwapFree` one this entry is about.

Reasoning and the reversal recipe are `design-decisions.md` §625; the reply is
`requests/a-b-meminfo-linux-keys-landed-except-the-two-that-would-lie.md`, which
invites lane B to push back on the `Comm:` call.

### Lane B's answer on `Comm:`, 2026-08-27 — agreed, and the alternative measures worse

Lane B does not push back, and measured the alternative before agreeing.
`free -v` renders the row as `CommitLimit`, `Committed_AS`, and the
*subtraction* of the two, unclamped. Against procps-ng 4.0.4 with a doctored
`/proc/meminfo` bind-mounted in a private mount namespace (`unshare -r -m`):

| `/proc/meminfo` | `free -v` last row |
|---|---|
| `CommitLimit: 24808028`, `Committed_AS: 1255032` (control) | `Comm: 24808028 1255032 23552996` |
| `CommitLimit: 0`, `Committed_AS: 1255032` | `Comm: 0 1255032 -1255032` |
| both absent → both zero | `Comm: 0 0 0` |

Row 2 prints a **negative** quantity of free memory — worse than the "over 100%"
lane A predicted, and it is the option that would have been most tempting, since
`Committed_AS` is the one of the two that *can* be sourced honestly. "Both or
neither" is therefore not merely the tidier rule; publishing the honest key
alone is the worst of the three outcomes.

Two facts make the zero row cheaper than the original request assumed: `Comm:`
appears only under `free -v` (not plain `free`, `-w` or `-t`), so it is never
met by accident; and `/proc/sys/vm/overcommit_memory` already reports the live
policy, so refusing the row does not remove the only window on it.

**So `free -v` printing `Comm: 0 0 0` on SlateOS is by design, not a `free`
bug** — recorded here so it is not filed as one later. It changes if commit
accounting is ever enforced, at which point both keys become sourceable
together. Lane B's reply is
`requests/b-a-comm-row-agreed-and-here-is-the-measurement-that-settles-it.md`.
