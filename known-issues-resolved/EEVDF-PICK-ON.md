### EEVDF-PICK-ON. EEVDF backend `pick_next` O(n) worst-case — RESOLVED 2026-07-15 (option (b) split-index rewrite)

**Status:** RESOLVED. `pick_next` is now amortised **O(log n)**, satisfying
CLAUDE.md's hard rule ("`pick_next` must be O(1) or O(log n) — never O(n)").
The secondary `min_vruntime`-approximation defect is fixed too. Kept below for
history and to document the design.

**Original problem (2026-07-01):** The run queue is a
`BTreeMap<(virtual_deadline, TaskId), EevdfEntry>` ordered by *deadline*, but a
task is *eligible* only when `vruntime <= min_vruntime`. The old `pick_next`
walked the tree from the front (earliest deadline) until it found the first
eligible task. Because the earliest-*deadline* tasks can be ineligible (higher
vruntime — e.g. a just-preempted task re-enqueued with accumulated vruntime but
an early deadline), that scan could walk past many entries: **O(n) worst-case**.
Secondary defect: `update_min_vruntime` derived its candidate from the
*earliest-deadline* task's vruntime, NOT the true minimum vruntime across the
queue, so the eligibility boundary itself was approximate.

**Fix implemented (option (b), split-index in safe std collections):** The
`tree` (deadline-keyed, all tasks) remains the source of truth, augmented by
two partition indexes plus a reverse index:
- `eligible: BTreeMap<(deadline, TaskId), ()>` — tasks with
  `vruntime <= min_vruntime`, ordered by deadline. `pick_next`'s Phase-1
  "earliest-deadline eligible task" is `eligible.iter().next()` = **O(log n)**.
- `ineligible_by_vrt: BTreeMap<(vruntime, TaskId), ()>` — the rest, ordered by
  vruntime. Its front is the smallest vruntime among ineligible tasks, which
  (a) feeds the true-minimum `min_vruntime` computation and (b) is the next
  candidate to promote as the floor rises.
- `deadlines: BTreeMap<TaskId, deadline>` reverse index so a task can be found
  in `tree`/`eligible` by id when promoting from `ineligible_by_vrt`.
- each `EevdfEntry` carries `is_eligible: bool` so removals (`dequeue`,
  `steal`, stale re-enqueue) hit the correct partition map in O(log n).

`update_min_vruntime` now sets the floor to the true minimum vruntime across
the ineligible set and the running task (only when `eligible` is empty, since a
non-empty eligible set means the floor is already at/above those vruntimes),
and stays monotonic. `rebalance()` drains `ineligible_by_vrt` from its front
into `eligible` while `front.vruntime <= min_vruntime`; because a waiting task's
vruntime is fixed and `min_vruntime` is monotonic, each task promotes **at most
once per residency**, so `rebalance` is amortised O(log n) per operation. It is
called after every mutation that can move the floor (`enqueue`, `dequeue`,
`tick`, `steal`, `pick_next`). Phase-2 fallback ("no eligible task → earliest
deadline overall") is `tree.iter().next()` = O(log n).

**Tests added (`eevdf::self_test`, all passing in boot self-test):**
"partition invariant holds across operations" (checks
`eligible.len()+ineligible_by_vrt.len()==tree.len()==nr_running` and
`is_eligible == (vruntime<=min_vruntime)` for every entry after each op),
"pick_next is deadline-correct under adversarial vruntime mix" (the exact case
that used to force the O(n) scan), and "min_vruntime tracks the true minimum,
not earliest-deadline". The pre-existing "weighted fairness" test was corrected
to assert on **CPU time (ticks consumed)** rather than pick *count*: with the
now-correct `min_vruntime`, a high-weight and low-weight task alternate picks
1:1, but the high-weight task runs a full slice while the low-weight one is
preempted early — weighted fairness correctly manifests as more CPU time, not
more picks. (The old pick-count assertion only passed by accident of the old
`min_vruntime` bug.)
