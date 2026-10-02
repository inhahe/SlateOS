## B-`df`-`du`-AND-`ls`-EACH-HAND-ROLLED-THE-SAME-BROKEN-SIZE-FORMATTER (lane B, 2026-08-19) — ✅ **FIXED 2026-08-19**

**In short:** the three utilities that print human-readable sizes — `df -h`,
`du -h`, `ls -lh` — each had their own private copy of "turn bytes into
`1.5G`", written as a chain of `{:.1}` format strings. All three were wrong,
in the same four ways, and every one of them had unit tests that asserted the
wrong answers. The most visible symptom: on a machine with terabyte disks,
`df -h` reported `1860.7G` where every other `df` in the world says `1.9T`.

**Where:** `human_size` in `src/bin/df.rs`, `src/bin/du.rs` and `src/bin/ls.rs`.
All three are now one-line calls to `coreutils::human::human_readable`; the
copies are gone.

**The four divergences**, measured against GNU coreutils 8.32 rather than
reasoned about:

| bytes | GNU | old `du`/`df` | what was wrong |
|---|---|---|---|
| 5 | `5` | `5B` | a bare count takes no suffix |
| 1025 | `1.1K` | `1.0K` | GNU rounds **up**; `{:.1}` rounds to nearest |
| 16777216 | `16M` | `16.0M` | the decimal drops once the mantissa hits ten |
| 5×10¹² | `4.6T` | `4768.4G` | the chain stopped at `G` |

**And a fifth, in `ls` only:** `human_size(1048576 - 1)` returned `1024.0K`.
One byte under a mebibyte is `1.0M` — 1023.999 K rounded up is 1024.0 K, which
is not a rendering and has to carry into the next prefix. The hand-rolled code
picked its prefix *first*, from a threshold comparison, then formatted, so it
had no way to carry. `1024.0K` is a string no `ls` has ever printed.

**Why it survived this long: the tests agreed with the code.** Every one of
these had coverage, and the coverage asserted the bug:

```rust
assert_eq!(human_size(0), "0B");        // GNU: 0
assert_eq!(human_size(1023), "1023B");  // GNU: 1023
assert_eq!(human_size(10 * 1024), " 10.0K");  // GNU: 10K
assert_eq!(human_size(2_500_000_000), "  2.3G");  // GNU: 2.4G
assert!(s.ends_with('K'));  // for 1 MiB - 1, where GNU says 1.0M
```

These were written by reading the implementation and writing down what it did.
That is the failure mode, and it is not fixed by writing *more* tests of the
same kind — the last one is instructive, because asserting only the suffix
looks like defensive testing while in fact checking the one thing that was
still right.

**Fix as landed.** All three now call `human_readable` with the option set GNU
passes for `-h` (`AUTOSCALE | CEILING | SI | BASE_1024`), which was itself
confirmed rather than assumed: `df --block-size=1` and `df -h` read together on
the same filesystems give 553.9 GiB → `554G` and 299.8 GiB → `300G` (so the
rounding is upward) and 1.818 TiB → `1.9T` (so the base is 1024). Every
replaced assertion was re-measured against GNU on a file of exactly that
length. `ls` keeps its six-column right-alignment, which is *not* GNU's
behaviour — real `ls` sizes the column to the widest entry — because that is a
layout question about `ls` rather than a rendering question about `human`, and
half-changing it would have been worse than leaving it.

**What makes the fix trustworthy** is `userspace/coreutils/tests/human_gnu.rs`:
36121 renderings measured from GNU across three instruments, all matching. See
`design-decisions.md` §338 for the fixture's construction and its limits.
