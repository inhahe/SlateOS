## §931 — The write benchmarks run with the file indexer LIVE, and that is now a choice rather than an accident

**Lane:** A
**Date:** 2026-09-12
**Decided by:** Claude (autonomous) — benchmark methodology, not OS behaviour.

**In short:** the file indexer is switched on while the performance benchmarks run, which
adds about 16 microseconds to every measured file write. Nobody chose that: a self-test
turns the indexer on and never turns it off. Having measured it, the decision is to leave
it on and say so in the results, rather than switch it off to make the numbers look
better.

**The mechanism.** `index::init` has exactly one caller in the tree — an interactive
`kshell` command — so a bare boot leaves the indexer dead and `on_file_changed` returns
after a predicate check. `index::self_test` calls `init` then `rebuild`, and
`rebuild_count` is never reset, so from that point the indexer is live for the rest of the
boot. `default_config` watches `/`, and `pathutil::path_in_subtree` short-circuits to
`true` for that root, so every path is watched. Confirmed empirically, not only by
reading: the scorecard now prints `indexer live=true (initialized=true, rebuilds=1,
entries=333)` — exactly one rebuild, which is the self-test's.

**Decision: keep it live.** Three reasons, in descending weight.

1. **All 152 recorded runs were taken this way.** Switching it off steps every
   `vfs_write_*` series at one commit, which the history ratchets read as a regression,
   and discards comparability with the entire recorded history for no gain in truth.
2. **A machine someone would actually use runs the indexer.** It is a feature, not a test
   artefact. Measuring writes without it would measure a configuration nobody ships.
3. **The cost is now reported rather than hidden**, so the number is interpretable. That
   was the actual defect — not the state, but that nothing said which state produced the
   number.

**The residual risk, named rather than glossed.** The indexer is live because a self-test
leaves it live, not because anything *ensures* it. Reorder the self-tests and the
benchmarks silently change what they measure. This decision does not fix that; it accepts
it, on the grounds that the scorecard line now makes such a change visible in the next
run's output instead of invisible. Ensuring the state deliberately would mean the
benchmark calling `init` + `rebuild` itself, which is *not* a no-op — a rebuild walks the
VFS and would change the index's contents and therefore `add_entry`'s cost. So the tidier
option is also the one that perturbs the measurement, which is why it was not taken.

**Alternatives considered.**

* *Restore the indexer's state in `index::self_test`*, the way `sockact.rs` was fixed the
  same week. Correct in isolation and wrong here: it would make the benchmarks measure an
  indexer that is off, which is less like a real system, while also stepping every series.
* *Measure both, as separate series.* Defensible, and the difference would be the indexing
  cost directly. Rejected for now as scorecard inflation: the `index` phase already
  reports that cost, and a second full write series per size would need the budgets to say
  which one they bind.

**What this commits us to.** `performance-targets.md`'s filesystem row already carries a
caveat that its "within 20% of Linux ext4" comparison is unlike-for-unlike because every
SlateOS write also reads back and hashes the old contents. The indexer belongs in that same
caveat: it is a second component ext4 does not have. Adding it is part of this decision and
not a separate task.
