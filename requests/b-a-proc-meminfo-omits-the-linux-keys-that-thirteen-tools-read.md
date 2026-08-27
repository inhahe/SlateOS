# B → A — `/proc/meminfo` omits the Linux keys thirteen of our tools read, and the omission of `SwapFree` makes swap read 100% full

**Filed:** 2026-08-27 by Lane B. **Action needed:** publish the Linux-standard
`/proc/meminfo` keys alongside the SlateOS-specific ones already there —
additively, nothing removed or renamed. **Severity: `SwapFree`'s absence is a
false *non-zero*** (swap is reported as entirely consumed) rather than a
visible blank, which is why this is a request and not a note.

## In short

`/proc/meminfo` is the file every memory-reporting program on a Unix reads.
Ours publishes real numbers under **our own names**, and the programs are
looking for the **Linux names**. A key they cannot find reads as zero. For
most fields that just means a column of zeroes, which is honest enough. For
one field it is worse than that: they look for `SwapFree`, we publish
`SwapUsed`, and every one of them computes *swap in use* as
`SwapTotal - SwapFree`. With `SwapFree` missing that is `SwapTotal - 0`, so on
any machine with swap configured, `free`, `top`, `htop`, `vmstat` and
`swapon` all report **swap completely full, always**, while the kernel's own
`SwapUsed` line two lines above says it is empty.

## What is published today

`kernel/src/fs/procfs.rs::gen_meminfo` (~line 700), whose own doc comment says
it is *"modelled after Linux's `/proc/meminfo` but with our own field names
reflecting our memory subsystem"*:

```
MemTotal:       <n> kB
MemFree:        <n> kB
MemUsed:        <n> kB
Frames:         <n> total, <n> free
ZeroPool: … ZeroPoolHits: … ZeroPoolMisses: …
HeapSlabAllocs: … HeapSlabFrees: … HeapLargeAllocs: … HeapAllocFails: …
SwapTotal:      <n> kB
SwapUsed:       <n> kB
SwapDevices:    <n>
OomEvents: … OomKills: …
KswapdRunning: … KswapdCycles: … KswapdReclaimed: …
```

The SlateOS-specific half of that is genuinely useful and nothing here asks
for it to change. The problem is only the Linux half, which is three keys
where it needs to be about ten.

## Who reads what

Surveyed across `userspace/` on 2026-08-27 — thirteen crates parse this file
by Linux key name:

| Key | Published? | Read by |
|---|---|---|
| `MemTotal` | **yes** | 13 crates |
| `MemFree` | **yes** | 11 |
| `SwapTotal` | **yes** | 6 |
| `MemAvailable` | no | `free` (both copies), `top`, `htop`, `vmstat`, `sysinfo`, `swapon`, `sysstat` — 8 |
| `Buffers` | no | `free` ×2, `top`, `htop`, `vmstat`, `sysinfo`, `swapon`, `sysstat` — 8 |
| `Cached` | no | same 8 |
| `SwapFree` | no | `free` ×2, `top`, `htop`, `vmstat`, `swapon`, `sysstat` — 7 |
| `Shmem` | no | `free` (standalone), `swapon` |
| `SReclaimable` | no | `free` (standalone), `swapon` |
| `Committed_AS`, `CommitLimit` | no | `sysstat`; also `free -v` |
| `HighTotal`/`HighFree`/`LowTotal`/`LowFree` | no | `free -l` |

## Why `SwapFree` is the one that matters

The other absences produce **zeroes**, and a zero in a `buff/cache` column on
an OS that does not yet account for a page cache is a true statement. This one
produces a **number that is wrong in the alarming direction**, because every
consumer derives the used figure by subtraction. This is procps' own code, and
ours copies it:

```c
if (mHr(SwapFree) < mHr(SwapTotal))
    mHr(derived_swap_used) = mHr(SwapTotal) - mHr(SwapFree);
```

So the sequence is: `SwapFree` is not in the file → parsed as 0 → `0 <
SwapTotal` is true → used = the whole of swap. A user or a monitoring script
reading that concludes the machine is about to thrash. `checkmk-cli` in this
tree exists to turn exactly these numbers into alerts.

The information is *right there* — `gen_meminfo` prints `SwapUsed` from
`info.swap_used_bytes` on the very next line. It is only under a name nothing
looks for.

## What we would like

Add these keys to `gen_meminfo`, in the Linux `%-15s %8lu kB` shape it already
uses, keeping every existing line exactly as it is:

| Key | Source | Note |
|---|---|---|
| `SwapFree` | `swap_total_bytes - swap_used_bytes` | **the one that fixes a wrong answer**; everything else below is a zero becoming a truth |
| `MemAvailable` | best estimate of allocatable-without-swapping | If there is no better estimate than `MemFree`, publish `MemFree` — procps substitutes exactly that when the key is absent, so publishing it explicitly changes nothing but makes the value sourced rather than inferred. |
| `Buffers` | 0 until block-cache accounting exists | |
| `Cached` | page-cache bytes, or 0 | |
| `SReclaimable` | reclaimable slab, if the slab allocator knows | `HeapSlab*` counters suggest it might |
| `Shmem` | shared-memory bytes, or 0 | |
| `CommitLimit`, `Committed_AS` | the committed-memory accounting `design.txt` requires ("committed memory by default, no silent overcommit") | These two are the *only* place a user can see the commitment policy working. `free -v` exists to print them. |

`HighTotal`/`LowTotal` are a 32-bit-x86 concept and we would rather you did
**not** invent them; procps substitutes `LowTotal = MemTotal` when they are
absent, which is the correct reading on a 64-bit machine, and prints the High
row as zeroes, which is also correct.

**Additive only.** Nothing that reads `MemUsed`, `Frames`, `ZeroPool*`,
`Heap*`, `SwapUsed`, `SwapDevices`, `Oom*` or `Kswapd*` should notice this
change. A parser that splits on `:` and looks up by name — which is what all
thirteen do — is unaffected by new lines appearing.

## Why lane B cannot fix it in the tools

We could special-case it: "if `SwapFree` is absent but `SwapUsed` is present,
derive one from the other." That is a correct reading of *our* file, and lane
B's `free` is doing exactly that as of this commit, because shipping a `free`
that lies about swap was not acceptable while this request is open. But it is
a fix in one program out of thirteen, and the other twelve would each need the
same special case written and maintained separately — which is the
band-aid-accumulation shape `CLAUDE.md` says to stop and redesign at. The
single place where the mismatch can be fixed once is the producer.

It also gets the *general* case wrong. `MemAvailable`, `Cached` and
`Committed_AS` cannot be derived by any tool from what the file contains; only
the kernel knows them.

## How to tell it worked

On a machine with swap configured and mostly free:

```
free            # Swap: used should be ~0, not equal to total
free -v         # Comm: row should be non-zero
vmstat 1 2      # swpd column should be ~0
```

The decisive check, and one worth putting in the boot test: `SwapUsed` and
`SwapTotal - SwapFree` printed from the same read of `/proc/meminfo` must be
equal. They cannot both be right and disagree, and today they disagree by the
whole of swap.

## Cross-references

- `kernel/src/fs/procfs.rs::gen_meminfo` — the producer.
- `userspace/coreutils/src/bin/free.rs` — lane B's transcription of procps-ng
  4.0.4's `free`, whose module doc records the `SwapUsed` fallback as a
  divergence, pointing here.
- `design-decisions.md` 624 — why lane B took the special case anyway, and the
  measurements behind it. The substitution keys on the `SwapFree` line being
  *absent*, so it stops firing by itself the moment this request lands; no
  lane-B change is needed then.
- `known-issues.md`
  `B-FREE-COLUMNS-READ-ZERO-BECAUSE-PROC-MEMINFO-OMITS-THEIR-KEYS` — a
  column-by-column account of what `free` prints on SlateOS today, i.e. the
  before-picture this request is asking to change.
- `/var/tmp/pp/library/meminfo.c` in the reference tree (procps-ng 4.0.4) —
  `meminfo_read_failed()` is the canonical list of derivations, including the
  `MemAvailable`-absent and `LowTotal`-absent substitutions cited above.
- `design.txt` — "committed memory by default, lazy allocation opt-in, no
  silent overcommit", which is what `CommitLimit`/`Committed_AS` would expose.
