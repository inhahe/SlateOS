## 1006. A command that does not work is deleted, not kept as a refusing stub

**Date:** 2026-09-07
**Lane:** B
**Decided by:** Operator (answering `open-questions.md` "2,288 of the 2,756
commands in `userspace/` report success for work they never did")

The audit counts a crate as *fabricating* when it performs no I/O of any kind
and nonetheless states a fact -- a measurement, a count, a `PASS`. 2,288 of
2,756 userspace crates qualify: 2,023 named `*-cli`, and 265 with plain tool
names (`bzip2`, `cal`, `docker`, `cmake`, `borg`) which are the more dangerous
group, because those are names a person actually types.

I proposed option A: delete the ~2,000 that can never work here (someone
else's proprietary product, or a cloud service we do not talk to), and keep
the rest as stubs that refuse and exit non-zero. **The operator went further,
and the stricter rule is the decision: delete every fabricating command, not
only the impossible ones.** A name that could genuinely be ported one day --
`pandoc-cli`, `sqlmap-cli`, `cal` -- is added back *when it is implemented*,
not before.

The reasoning is the same one this entry used to argue for refusing stubs, and
it turns out to point the other way: **a command's existence is itself a
claim.** It is a claim to the user who sees it in `PATH` or in completion, and
it is a claim to every script and installer that probes with `command -v`
before deciding what to run. A refusing stub answers "yes, that exists" to the
probe and then fails at the point of use, which is strictly worse than
answering "no" up front. The stub only helps if the failure message is read by
a human, and probes are not humans.

**What changes for a user:** ~2,288 names stop existing. Typing one gives the
shell's `command not found`, which is true. The workspace loses those crates
and builds faster.

**The ratchet, which follows from the answer:** once the deletion has landed,
`scripts/audit-cli-fabrication.py` is pinned the way
`scripts/scan-orphan-modules.py` is -- the count is fixed at its new floor and
the gate fails if it ever rises. Pinning before the deletion would have pinned
a number about to change by two thousand; pinning after is what stops the next
bulk generation from putting it all back.
