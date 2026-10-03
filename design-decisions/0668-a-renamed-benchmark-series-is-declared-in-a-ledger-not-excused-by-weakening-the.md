## §668 — a renamed benchmark series is declared in a ledger, not excused by weakening the guard

**Date:** 2026-09-01
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** Every benchmark run appends its numbers to one long-lived file,
keyed by the benchmark's name. A test refuses to build if a name that was
measured last run is missing this run, because a benchmark that quietly stops
being measured takes its whole regression coverage with it. But I had just
*renamed* a benchmark on purpose — `vfs_readdir` became `vfs_readdir_32`,
because it now times a different thing and the old numbers are not comparable —
and to that test a deliberate rename and a silent loss look identical. Rather
than soften the test, renames are now written down in
`bench/renamed-series.txt`, and the test consults it. An undeclared
disappearance still fails exactly as before.

### The collision

Two rules, each correct, that had never met:

- **A redefined benchmark must get a new name.** `bench/history.jsonl` keys a
  series by name and nothing else; there is no "same series, new definition"
  field and there cannot usefully be one, because each record is a ~9-minute
  QEMU boot that measured whatever the code did on that commit and cannot be
  regenerated. Diffing a redefined benchmark against its old records compares
  two different measurements under one name. `kernel/src/bench.rs` enforces this
  mechanically: a `const _` assertion fires if the entry count changes without
  the series name changing.
- **A name that vanishes fails the build.** `test_history_still_loads` compares
  the last two records and asserts `removed == []`. Its comment says why the
  additions half is only reported while this half is fatal: the loss is silent,
  and "a check that fires on every legitimate change gets deleted — and deleting
  this one would take the `removed` half with it, which is the half that
  actually protects the data."

The second rule fires on obeying the first. Note the timing that hid this: the
guard reads the *committed* history, and a `--bench` run appends its record
after its own tests have run, so the rename passed the run that introduced it
and failed the next one.

### What was rejected

| Option | Why not |
|---|---|
| Drop `removed == []`, keep the report | This is the assertion, not a wrapper around it. The suite would keep passing while coverage silently left the tree — the exact failure the test was written for. |
| Allow-list the retired name | An allow-list says "stop looking at this name". It cannot distinguish a rename from a deletion, so it grants the same excuse to the case the guard exists to catch. |
| Special-case `vfs_readdir` in the test | Solves one rename by hand and leaves the next one to rediscover the whole problem, with a green build in between while somebody argues about it. |
| Compare against an older record that predates the rename | Moves the baseline for every other benchmark too, to dodge one name. The diff would then be measured against the wrong run — the failure `load_history`'s sort exists to prevent. |

### What was done

`bench/renamed-series.txt`: `old -> new`, one per line, with the date, the
commit and the reason in a comment above each entry. `load_renames` parses it
and `resolve_rename` follows chains; `unexplained_removals` splits a vanished
name into *undeclared* (no entry at all) and *misdeclared* (an entry that lands
nowhere), which need different fixes and so are reported separately.

Three properties keep this a declaration rather than a suppression:

1. **The excuse is conditional on the successor being measured in the very
   record under test.** The entry asserts "this series continues under that
   name". If the successor is not there either, the coverage really is gone and
   the guard fires anyway — naming the successor, which is the thing the reader
   has to go and look for.
2. **Absence of evidence excuses nothing.** An empty ledger, a missing file, a
   looping chain: all leave the removal unexcused. Only a positive declaration
   plus a present successor gets through. This is the same asymmetry
   `split_by_layout` uses, and for the same reason — an excuse granted by
   absence silences the check in the ordinary case.
3. **A retired name may not be scored again.** `test_committed_rename_ledger`
   fails if any ledger source name reappears in the latest record. Reusing a
   retired name would graft new numbers onto the orphaned history of something
   else, and it is also the only way a years-old entry could excuse a genuine
   removal.

The ledger raises `ValueError` on a malformed line rather than skipping it, the
opposite of `load_history`'s per-line recovery. The files differ in what a
dropped line costs: `history.jsonl` is machine-appended and irreplaceable, so
partial recovery beats an exception, while this one is hand-written and
regenerable from `git log` — and a silently-dropped line here would leave a
rename looking declared when it is not.

Entries are never pruned. Each is consulted for exactly one run (the pair of
records straddling the rename) and is inert afterwards, but it stays as the
answer to "why does this name in the old history just stop", which a reader
would otherwise have to reconstruct from `git log`.

### The rule that comes with it

Declare the rename in the **same commit** as the rename. A declaration added
afterwards, once the build has already gone red, is indistinguishable from a
rename excused afterwards — and the whole value of the ledger is that it is
written by someone who still intends the rename, not by someone unblocking
themselves.
