## 626. `dd` allocates its buffer twice on purpose, and refuses the I/O flags this platform cannot honour — which is upstream's own rule, not ours

**Date:** 2026-08-27
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** `dd` is the tool that copies raw bytes between files and devices —
the one people use to write a disk image to a USB stick. Ours is now a
line-by-line copy of GNU's, following the same transcription policy set in 622
and 623 for `cal`. Four choices in it are genuine tradeoffs rather than
copying. First, `dd bs=N` allocates its buffer **twice**: once as a throwaway
probe that is allowed to fail, then again for real — because Rust has no single
call that both reports failure politely *and* leaves the memory untouched, and
`dd bs=10000000000` needs both. Second, the `iflag=`/`oflag=` names that ask
for a kernel feature this OS does not have (`direct`, `dsync`, `noatime`, …)
are **refused with an error** rather than accepted and quietly ignored — which
turns out to be exactly what real GNU `dd` already does on any machine lacking
those features. Third, there is no `SIGUSR1` handler, because this OS does not
have Unix signals at all. Fourth, two shared library modules were widened for
`dd`'s benefit rather than letting `dd` keep private copies — which is a change
every other command links against, so it needs justifying.

### Why the buffer is allocated twice

`dd bs=99999999999999` is a plausible typo — one keystroke off a real size.
GNU answers it with a sentence and exit status 1:

```text
dd: memory exhausted by input buffer of size 99999999999999 bytes (91 TiB)
```

That is a *diagnostic*, so our version must not abort. But the size also has to
be obtained **lazily**, and no single Rust call does both:

| Call | Fails politely? | Leaves pages untouched? |
|---|---|---|
| `vec![0u8; n]` | no — aborts the process | yes — specialises to `alloc_zeroed` |
| `try_reserve_exact(n)` + `resize(n, 0)` | yes | **no** — `resize` writes a zero into every byte |

The second row is the trap, and it is not theoretical. `dd if=/dev/null
bs=9999999999` asks for ten gigabytes; GNU completes it instantly and prints
`0+0 records in`, because `malloc` hands back untouched address space and the
buffer is a *read target* that on `/dev/null` is never read into. Zeroing it
would fault in all ten gigabytes and reach the OOM killer on a copy that is
supposed to be free. Measured against GNU on the dev machine: instant, no
resident growth.

So `alloc_buffer` runs the fallible call as a **probe** and drops it, then
returns the lazy one:

```rust
if let Ok(n) = usize::try_from(size)
    && let Some(n) = n.checked_add(extra)
    && {
        let mut probe: Vec<u8> = Vec::new();
        probe.try_reserve_exact(n).is_ok()
    }
{
    return Ok(vec![0u8; n]);
}
```

The probe costs one anonymous mapping and its immediate unmapping — it is never
written to either — and it is what converts an abort into the sentence above.

*The alternative considered and rejected* was `alloc_zeroed` through
`std::alloc` directly, wrapped in a `Vec::from_raw_parts`. That is one
allocation instead of two and is exactly what upstream's `malloc` does. It was
rejected because it needs `unsafe` in a binary that otherwise has none, and the
safety argument has to cover the layout, the capacity, and the drop — three
invariants to buy back one `mmap`/`munmap` pair on a path that runs **once per
process**. The probe is the cheaper thing to be wrong about.

The reported size deliberately excludes `conv=swab`'s extra byte, as upstream's
does — the number in the message is the number the user typed.

### Why unhonourable flags are refused rather than ignored

The previous `dd` in this tree accepted six operands and rejected everything
else, on the stated principle that "a `dd` that accepts `conv=notrunc` and
truncates anyway is worse than one that refuses the request, because the caller
has no way to find out". The rewrite implements nearly all of it, but that
principle still decides the leftovers: thirteen `iflag=`/`oflag=` names ask for
an `O_*` bit this platform has no equivalent for, and accepting them silently
would mean `dd iflag=direct` reporting success while going through the page
cache — the one thing the caller used the flag to prevent.

The important discovery is that this is **not a divergence at all**. Upstream's
own acceptance loop is:

```c
if (!(operand_matches (val, entry->symbol, delim) && entry->value))
  continue;
```

— a zero-valued table entry falls through to the "invalid flag" error exactly
as an unknown name does. GNU builds its table from macros that are `0` when the
platform lacks the feature, so **stock GNU `dd` on Linux today already rejects
`iflag=binary`, `iflag=text`, `iflag=cio` and `iflag=nolinks`**, measured. Our
table is the same table with more zeros in it, and the behaviour follows
mechanically. Carrying the names at value 0 rather than deleting them is what
makes that true; deleting them would produce the same message by a different
route and would silently stop tracking upstream if a name were ever added.

Sixteen of the twenty deliberate differences in `dd-diff.sh` are the resulting
per-name rows (nine `iflag=`, seven `oflag=`). The other four are `--help`,
`--version` and their two abbreviations.

### Why there is no `SIGUSR1`

GNU `dd` prints its statistics on `SIGUSR1` and re-prints them on a fatal
signal. Neither is here, and the corresponding `--help` paragraph is absent.
`design.txt` rules out Unix signals for process control on this system — that
is an architectural requirement, not a shortcut — so there is no signal to
catch. `status=progress`, which is implemented, covers the reason people reach
for `SIGUSR1` in the first place.

### Two shared modules grew a function rather than `dd` growing a copy

`dd` needed two things the library did not expose, and in both cases the choice
was between widening the shared module — which touches code every other binary
links — and writing a private version inside `dd`. The shared module won both
times, for the same reason: the private version would have been a *second*
implementation of something already implemented correctly, and the two would
have drifted.

**`xnum::xstrtoumax_end`.** gnulib's `xstrtoumax` has a `char **ptr`
out-parameter reporting where the scan stopped. This port dropped it, because
no caller in the tree used it. `dd` cannot manage without it, and not for
convenience: two of its operand rules are *defined* in terms of that position.
`count=1B` means bytes rather than blocks, and `B` is deliberately absent from
`dd`'s suffix list, so the way `dd` detects it is to see `InvalidSuffix` with
the end sitting on a `B` — then check the bytes on either side, so that `1kB`
(where `B` was already consumed as the base-switching suffix) stays valid and
`1kBB` stays invalid. And `bs=2x512` is a product, parsed by finding the `x` at
the end position and re-entering the same function on the remainder. Neither
test is expressible from the return value alone.

The alternative was a private integer parser in `dd.rs`. Rejected: `dd`'s
numbers are *the* place where a parsing bug destroys data — `bs=1MB` silently
becoming `bs=0` is defect 2 of
`known-issues.md` → `B-dd-DESTROYS-THE-OUTPUT-FILE-WHEN-seek=-IS-GIVEN` — so
the parser `dd` uses should be the one with the most callers exercising it, not
the one with one. The cost is a three-element tuple on a function whose other
callers want two; that is paid by keeping `xstrtoumax_base` as a thin wrapper,
so no existing call site changed.

**`filekind::borrowed`.** `borrowed_stdin` became `borrowed(fd)` for 0, 1 and 2.
`dd`'s `seek=`, its `conv=sparse` and its closing `ftruncate` all have to reach
**standard output** when no `of=` was given, because `dd seek=10 > img` is the
ordinary spelling and must not be a different program from `dd seek=10 of=img`.
Only three descriptors are nameable: on Windows a descriptor number is not a
handle, so the mapping goes through the three `std::io` accessors and there is
no fourth. Anything else returns `None` rather than fabricating a handle.

### What `dd-diff.sh` compares, and where the elision stops

`dd` is three programs wearing one name — a converter, a record counter, and a
file writer — and each of the three hides its bugs on a different channel. So
the harness compares **four** things per case, where every other harness in
`scripts/` compares two:

| Channel | The bug class it is the only witness to |
|---|---|
| stdout bytes | the conversions (`conv=ascii`, `swab`, `block`, …) |
| scrubbed stderr | the record accounting — `0+1 records in` vs `1+0 records in` is invisible in the output bytes |
| exit status | warning-vs-error, e.g. `skip=` past EOF |
| a directory manifest (each file's size, block count, bytes) | everything about `of=`, including the seek-truncation defect, which changes *no* other channel |

The scrub boundary is a decision. `dd`'s statistics line ends in a duration and
a rate, which are nondeterministic, so something must be elided — but eliding
the whole line would have been one character shorter to write and would have
certified nothing. The scrubber cuts only from `copied,` onward:

```sh
scrub() { sed -e 's/ copied, .*$/ copied, ELIDED/'; }
```

so `10000 bytes (10 kB, 9.8 KiB) copied, ELIDED` still compares the byte count
and *both* human-readable renderings — which come from the shared
`coreutils::human` module and are therefore the part most likely to be wrong.

`status=progress` needs a second, blunter scrubber, because the number of
in-flight progress lines depends on how the scheduler felt: it normalises every
`copied,` line to one constant and then collapses **adjacent** duplicates. A
burst of any length therefore compares equal to a burst of any other length,
but a burst of *none* — or one shaped differently — still differs, and the final
report survives because it is not adjacent to the burst.

*Proving the four channels are live.* Zero differences on a first run is
evidence of a harness that compares nothing just as readily as of a correct
program, so each channel was tamper-tested by wrapping GNU `dd` in a shim that
corrupts exactly one of them:

| Shim | Result over the 339 cases |
|---|---|
| append a byte to stdout | 0 passed, 339 differed |
| discard stderr | 4 passed, 335 differed |
| create one extra file | 0 passed, 339 differed |
| always exit 7 | 0 passed, 339 differed |

The four stderr survivors are the cases that print nothing on stderr to begin
with, so suppressing it changes nothing — which is the expected residue, not a
gap. With no shim, `OURS=/usr/bin/dd` turns all 20 deliberate differences into
XPASSes and keeps the 339 agreements, confirming they are not artefacts of
comparing GNU with itself.

### How to reverse

The double allocation: replace `alloc_buffer`'s body with a single
`try_reserve_exact` + `resize`, accepting that a large `bs=` faults in the whole
buffer, or with an `unsafe` `alloc_zeroed` if the two mappings ever measure.
The flag rejection: give the thirteen zero-valued table entries real values
once this platform grows the corresponding `open` behaviour — no code changes,
the loop condition does the rest — and delete the two `xfail_case` loops at the
foot of `dd-diff.sh` as each becomes an XPASS. The scrub boundary: widen
`scrub()` to `s/^.*copied.*$/ELIDED/` only if a genuinely nondeterministic byte
count ever appears before `copied,`, which would mean a real bug elsewhere.
