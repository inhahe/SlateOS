### TD-OILS-TRAP-P-OPERANDS-AS-FILTER. `trap -p SPEC…` filtered the trap table instead of walking its operands, so it reordered its output, swallowed duplicates and ignored a bad signal name — 2026-08-03 — ✅ **RESOLVED 2026-08-03**

**Where:** `userspace/oils/src/interp.rs` — `Shell::trap_print`.

**The bug.** `trap -p` has two shapes that are not the same walk. With no
operands bash lists the whole table in signal order; with operands it walks the
*operand list*. osh modelled the operands as a `filter` over the sorted table
instead, which got three things wrong at once:

```sh
trap ':' INT; trap 'x' USR1
trap -p USR1 INT   # bash: USR1 then INT.  osh: INT then USR1 (signal order).
trap -p INT INT    # bash: two lines.      osh: one (a set-membership test).
trap -p INT BOGUS  # bash: status 1 + "BOGUS: invalid signal specification".
                   # osh: status 0, silence — `filter_map` dropped the bad name.
```

The silent-skip is the worst of the three: a script that misspells a signal in
`trap -p` got no indication at all.

**The fix.** Split the two shapes. The no-operand path still sorts the table;
the operand path walks `specs` in order, emits a line per operand that has a
trap, prints nothing for one that has none, and reports one that names no signal
with status 1. The output is flushed before each complaint so the two streams
interleave the way bash's do. Covered by
`trap-p-walks-its-operands-rather-than-filtering-the-table.sh` and a lib test.
