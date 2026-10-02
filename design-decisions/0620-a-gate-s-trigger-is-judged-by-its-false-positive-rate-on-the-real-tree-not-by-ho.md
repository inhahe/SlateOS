## 620. A gate's trigger is judged by its false-positive rate on the real tree, not by how sound it sounds

**Date:** 2026-08-27
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** we added an automatic check that stops the build if a self-test
compares two separate readings of a counter that is moving on its own — the
mistake that had just panicked the kernel. The first version of that check
sounded perfectly reasonable and flagged 297 places, of which about five were
real. Rather than ship it with a 292-line "known exceptions" file, we made the
check smarter until it flagged 7. The decision recorded here is that a gate
must be measured against the actual codebase before it is adopted, and that a
large exception list is evidence the trigger is wrong, not evidence the
codebase is bad.

**Context.** `kernel/src/fs/irqstat.rs` panicked a green tree over a single
timer interrupt: rung 3 took one snapshot of the interrupt counters, rung 6
took another, and asserted the two were equal. The same shape existed in three
other modules the same day. Per this project's own gate discipline — four
instances of one omission, each invisible from inside the function that has it
— that calls for a checker rather than a sweep.

**The choice.** Writing the checker required picking a trigger, and there were
two candidates:

| Trigger | *What changes:* |
|---|---|
| A self-test **reads** one live counter twice | Cheap regex. **297 findings, ~5 real.** |
| A self-test **compares two different readings** of one live counter with `==` | Needs local dataflow. **7 findings, 5 real.** |

The first was chosen initially, on an explicit and wrong estimate: that
proving a comparison needed dataflow, that dataflow was out of reach for a
script, and that the weak trigger would cost little because the kernel had
"about a dozen" such sites. Running it produced 297 — a factor of
twenty-five — because 173 were `elapsed_ns`, which every duration measurement
reads exactly twice *on purpose*, and 116 were `cpu_count` read once per loop
in tests that loop over CPUs more than once.

**Why the weak trigger was rejected rather than ledgered.** The project already
has the machinery to carry known exceptions with reasons (296), and it would
have been a five-minute job to dump all 297 into it. That is precisely what
makes it the wrong move: a ledger of 297 entries is a rubber stamp by
construction. Nobody re-reads it, every new entry is waved through by analogy
with the 296 above it, and the one line that matters is invisible. 299 says a
gate's trigger is part of its rule; a trigger that is wrong 98% of the time is
not a rule with exceptions, it is noise with a ledger attached.

**What was built instead.** Local dataflow, which turned out to be modest:
bind live-counter reads to the locals they flow into, and ask whether an
equality assertion has two *different* reads of one counter on opposite sides.
Three refinements were then forced by real false positives, each of which is
its own small lesson:

* **A wrapper propagates a counter only if the read reaches its return value.**
  Timestamping is everywhere — `certmgr::import_cert` reads the clock to stamp
  `created_ns` and returns a `CertId` — so "touches the counter anywhere"
  made every value the function ever returned a clock reading, and comparing
  two file paths was reported as comparing two clocks.
* **A tail expression's value comes from the closure, not the call.** Kernel
  state accessors are overwhelmingly `with_state(|s| { ...; Ok(x) })`; a rule
  that cannot see through that idiom cannot see most of them.
* **`if let` binds from its scrutinee, not from the whole `if` body.** Treated
  as an ordinary `let`, the binding absorbed the entire block, including two
  clock reads that had nothing to do with it.

**The alternative not taken: tolerate instead of restructure.** For the
original panic, relaxing `==` to `>=` would have made it green immediately.
Rejected: the rung's property genuinely *is* equality — the line table must sum
to the aggregate it came from — and an inequality would stay green for a table
that double-counted a vector. Where a snapshot is impossible and the counter is
monotone, the fix used instead is to *bracket*: read before and after and
assert membership in that range. That is exact rather than tolerant — when
nothing moves, the range is a single value and it is the original equality.

**Cost accepted.** Two false positives remain, both from one function
(`sysfs::read_file` is a path-dispatching match, so every string it returns
carries every counter any arm reads). They are in the ledger with the reason.
Keeping that over-approximation is what keeps the same function's two *real*
findings visible, which is the trade a gate should make.

**What it caught that review had not.** `cpu_hotplug::self_test`, a fifth
instance nobody had looked at — and chasing why its two readings could differ
surfaced a genuine defect in AP bringup: `smp::init` waits for APs on a
*bounded* ~50 ms spin and then proceeds regardless, so a straggler is never
recorded by the hotplug framework at all
(`A-CPU-HOTPLUG-INIT-SNAPSHOTS-A-CPU-COUNT-THAT-CAN-STILL-GROW`). The test was
asserting something the code does not guarantee, and the assertion was the only
thing pointing at the gap.
