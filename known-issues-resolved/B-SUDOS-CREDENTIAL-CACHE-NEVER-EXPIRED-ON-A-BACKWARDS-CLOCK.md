## B-SUDOS-CREDENTIAL-CACHE-NEVER-EXPIRED-ON-A-BACKWARDS-CLOCK, AND `sudo -k` DID NOTHING UNDER `timestamp_timeout=-1` (lane B, 2026-09-12) -- FIXED

**In short:** `sudo` remembers that you typed your password, so a second `sudo`
soon after does not ask again. Two ways that memory outlived what it was meant
to mean. If the system clock ever steps **backwards**, the remembered moment is
in the future and the memory never expires. And `sudo -k`, which exists to
forget it deliberately, did not work at all when the administrator had
configured the memory never to expire on its own.

**Where.** `userspace/sudo/src/main.rs` -> `check_timestamp`, now split so the
decision is in `timestamp_is_fresh(content, now, timeout)` with the clock as a
parameter.

### Defect 1: a future timestamp

```rust
now.saturating_sub(ts) < timeout
```

With `ts` after `now`, `saturating_sub` yields **0**, `0 < timeout` is true, and
the prompt is skipped -- for as long as the clock takes to catch up. Nothing
fails anywhere: the read succeeds, the parse succeeds, and the subtraction
answers a question nobody asked.

A backwards clock step is ordinary -- an NTP correction, an RTC read at boot
before the network is up, a restored VM snapshot. Real `sudo` treats this as
`TS_FATAL`, "timestamp too far in the future".

### Defect 2: `sudo -k` under a never-expiring timeout

`invalidate_timestamp` writes `0` "to invalidate without removing". Under any
finite timeout 0 is 1970 and long expired, so it worked. Under
`timestamp_timeout=-1` **nothing** expires, and the `timeout == u64::MAX` arm
returned `true` before the age was ever considered -- so the sentinel came back
fresh. `sudo -k` was a no-op for exactly the configuration that most needs it:
the one where the credential otherwise lasts the whole session.

The reader now honours the sentinel, rather than the writer being asked to pick
a different number, because the reader is what has to agree with it.

### What this says about the method

**The error arms were never the problem.** This function's doc comment is a
careful argument that every failure answers `false`, and it ends:

> Checked on 2026-09-10 during a sweep for predicates that answer `false` on
> failure -- this one is correct as written and is noted so the next sweep does
> not have to re-derive it.

That note was accurate about everything it examined and would have stopped the
next reader looking. Both defects are in the **success** path. It is the second
guard in two days of this shape -- `doas`'s `user_in_group` was the first, and
its doc comment was likewise written to warn about fail-open and likewise
guarded the wrong arm.

So the sweep's question -- *what does it do when it fails?* -- is not enough on
its own. The one that found these is: **what fact does it actually establish,
and is that the fact the caller needs?** Here the caller needs "did this user
authenticate within the last N seconds"; the function established "is the
recorded number less than N away from the clock", which is a different
statement whenever the number is in the future or is a sentinel.

### And a near miss worth recording

The first draft of the invalidation test asserted
`timestamp_is_fresh("0", now, u64::MAX) == true` -- **the broken answer, written
down as the expected one**. I caught it while re-reading what I had just
asserted rather than that it passed. That is how a defect becomes a fixture,
and a green suite then defends it.

Six tests, where the decision previously could not be tested at all: the clock
and the file were both welded in.
