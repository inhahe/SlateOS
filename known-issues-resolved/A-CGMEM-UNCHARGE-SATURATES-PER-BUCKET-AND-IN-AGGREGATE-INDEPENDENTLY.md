## `A-CGMEM-UNCHARGE-SATURATES-PER-BUCKET-AND-IN-AGGREGATE-INDEPENDENTLY` (lane A, 2026-08-26) — **fixed** the same day

**In short:** `cgmem` tracks a cgroup's memory as one total (`usage`) plus a
breakdown of that same memory into two buckets (`rss` for a program's own
pages, `cache` for cached file data). Charging always keeps the two in step.
Un-charging does not: it subtracts from the total and from one bucket
*separately*, and each subtraction stops at zero on its own. Two shell commands
are enough to leave a cgroup reporting a total smaller than one of its own
parts.

**Where.** `kernel/src/fs/cgmem.rs`, `record_uncharge` (~line 176):

```rust
c.usage_pages = c.usage_pages.saturating_sub(pages);
if is_cache {
    c.cache_pages = c.cache_pages.saturating_sub(pages);
} else {
    c.rss_pages = c.rss_pages.saturating_sub(pages);
}
```

`record_charge` maintains the invariant `usage_pages == rss_pages +
cache_pages`: it adds `pages` to `usage_pages` and to exactly one of the two
buckets, and `swap_pages` is never written by either function. `record_uncharge`
subtracts from `usage_pages` and from one bucket, but the two `saturating_sub`
calls floor independently, so whenever the named bucket holds fewer pages than
are being un-charged the total drops by more than the breakdown does.

**Reproduce.**

```
cgmem init
cgmem create demo 1000
cgmem charge 1 100 rss      → usage=100 rss=100 cache=0
cgmem uncharge 1 50 cache   → usage=50  rss=100 cache=0
cgmem list                  →   [1] demo  usage=50/1000 rss=100 cache=0 swap=0 oom=0
```

The cgroup now reports 50 pages of memory of which 100 are resident.

**Why it matters beyond the arithmetic.** `usage_pages` is the number
`record_charge` compares against `limit_pages` to decide whether the cgroup has
exceeded its ceiling. An un-charge that deflates the total without deflating
the breakdown therefore buys the cgroup headroom it does not have: after the
sequence above the cgroup may charge another 950 pages before it trips its
limit, while genuinely holding 100. The error is in the permissive direction,
which is the direction that does not announce itself.

**Not reachable only through a typo.** This was noticed while surveying
`cmd_cgmem` for the guessed-value burn-down, where the `[rss|cache]` operand
was read with `unwrap_or("rss") == "cache"` and so turned any misspelling into
`rss`. But the sequence above types `cache` correctly; the defect is in the
subsystem, not in the parse, and clearing the parse does not clear it.

**Fixed.** Un-charge no more from the aggregate than actually left the bucket,
so the two can never diverge:

```rust
let actual = if is_cache {
    let n = pages.min(c.cache_pages);
    c.cache_pages -= n;
    n
} else {
    let n = pages.min(c.rss_pages);
    c.rss_pages -= n;
    n
};
c.usage_pages -= actual;   // == rss + cache still holds
```

`actual <= the bucket <= usage_pages` by the invariant, so the aggregate cannot
underflow and the invariant holds afterwards.

**The alternative that was rejected**, and it is a real trade-off rather than
an obvious call: refuse an un-charge larger than the bucket with
`KernelError::InvalidArgument`. That has a genuine argument behind it —
silently un-charging less than asked is itself a quiet substitution of a number
the caller did not supply, which is the very shape §600 exists to stamp out.
The clamp wins because `record_uncharge` has exactly one honest job, keeping
the two views of the same memory consistent, and a caller who over-un-charges
has *already* lost track of what it charged; refusing leaves that caller's
accounting wrong with no way to resynchronise, whereas clamping repairs the
invariant on the spot. The §600 objection does not really apply either: the
substituted number is not a guess about what the user meant, it is the
arithmetic truth about how many pages were actually there.

**Regression test.** `cgmem::self_test` test 9 covers both halves — un-charging
a bucket that is empty (which used to deflate the aggregate and nothing else)
and un-charging more than the right bucket holds — and asserts `usage_pages ==
rss_pages + cache_pages` directly.

**Not a regression.** True since `record_uncharge` was written.
