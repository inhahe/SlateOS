## §235 — A reachability map may escalate a finding, never dismiss one, because "I found no call path" is not "there is no call path"

**Date:** 2026-08-19
**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** When the benchmark harness flags a slowdown, it is useful to know
whether the code change under test even *runs* inside the benchmark that got
slower. The obvious use of that knowledge — "the change doesn't touch this
benchmark's code, so the slowdown must be a build artifact, drop it" — is the
one use that is unsafe, because a search of the source can miss a call path that
really exists, and dropping a real slowdown is the expensive mistake. So the map
is wired the other way round: nothing is ever excused *because* of it, and it
only ever adds a warning on top of an excuse that was already granted on other
evidence. Every gap in the map therefore produces silence, which changes no
verdict.

### The problem, and why it stayed open

`known-issues.md`'s standing fix list for the layout-artifact problem had three
parts. Parts (1) and (3) landed; part (2) — *"label cross-image movements
`MOVED (image changed)`, not `REGRESSED`, unless the changed files plausibly
reach the benchmark"* — sat blocked for weeks behind what looked like a tooling
gap ("we need a benchmark-to-source reachability map"). It was not a tooling
gap. It was the direction of the clause.

Read it precisely: a movement is *demoted* out of `REGRESSED` when the changed
files do **not** plausibly reach the benchmark. The predicate that fires is
non-reachability. To demote correctly, the tool must establish that no path
exists from the diff to the benchmark — and no static analysis available here
can establish that. `bench.rs` calls into the kernel through helper functions,
trait objects and generic instantiations; `-C lto` and inlining dissolve the
call graph the source suggests. A grep-grade map that finds nothing has two
indistinguishable explanations: there is no path, or the map cannot see it.

The two errors are not symmetric:

| The map says | Truth | Consequence |
|---|---|---|
| unreachable | unreachable | correct demotion |
| **unreachable** | **reachable** | **a real regression is relabelled a build artifact and disappears** |

The bottom row is the whole reason the harness exists. A tool whose failure mode
is *silently deleting the finding it was built to protect* is worse than no
tool, because it also removes the reader's suspicion. So part (2), as written,
was not blocked on effort — it was unimplementable at any effort, and the
correct response was to reverse it rather than keep waiting for a map good
enough to satisfy it.

### The decision

Reachability is used in exactly one direction: **to escalate.**

- A movement is excused only by the *measured* layout band (§234), on positive
  evidence, exactly as before. The map is not consulted to excuse anything.
- *After* an excuse is granted, the map runs. If the diff provably edits a file
  belonging to a subsystem the benchmark demonstrably enters, the report adds a
  warning: placement and the diff are now **both** live explanations, and this
  measurement cannot tell them apart. Read the diff.
- Every uncertainty resolves to silence: no map entry for the benchmark, a
  `git diff` that failed, an unreadable `bench.rs`, no base commit. `None`
  escalates nothing, and silence leaves the verdict exactly as §234 set it.

Now invert the table. A *found* call path is positive evidence — the map only
claims a path when it can point at one. A *missing* one produces no output. The
expensive row is gone: the map can no longer delete a finding, only annotate
one.

### What the map actually is, and why its sloppiness is acceptable

`benchmark_subsystems()` in `scripts/bench-history.py` scans `kernel/src/bench.rs`
for `score("name", …)` call sites, collects the `mod::` path prefixes appearing
in the enclosing function, and resolves each to a file or directory under
`kernel/src/`. `changed_paths()` asks git what the comparison's base commit
changed. `diff_touches()` intersects them.

This is a crude map, and it is crude in two directions — both of which are safe
under escalate-only semantics:

- **It under-covers**: 67 of 86 benchmarks get an entry. The other 19 name no
  module the scanner recognises. Under the old direction, an uncovered benchmark
  would have been auto-demoted (unreachable by default) — catastrophic. Under
  this one, it simply never escalates.
- **It over-attributes**: `vfs_stat_root` picks up `sync` and `lockdep` because
  those module paths appear in its enclosing function. Under the old direction,
  over-attribution would *suppress* demotions, which is at least safe; here it
  causes an occasional warning that sends a reader to a diff that turns out to
  be irrelevant. That costs a minute of reading. The opposite error costs a
  shipped regression.

Neither flaw can change a verdict. That is the property being bought, and it is
the reason a map this rough is worth having at all — its accuracy determines
only how often a *true* warning is printed, never whether a *false* excuse is
granted.

### Alternative considered: build a real call graph

`cargo`'s MIR, or `llvm-cfg` over the built object, could yield a genuine call
graph and would answer non-reachability far better than grep does. Rejected —
not on cost, but because it does not change the argument. LTO and inlining mean
even a real call graph is a graph of *one particular build*, and the claim
needed for demotion is about the semantics of the source. A better map would
narrow the bottom row of the table without eliminating it, and a rare silent
deletion of a real regression is worse than a frequent one, because nobody is
looking for it. The escalate-only framing removes the row outright, at which
point map quality stops being a correctness question and becomes a
signal-to-noise one — where grep is adequate.

### Consequences

- `known-issues.md`'s part (2) is closed, in reversed form; the wording there
  now records the reversal rather than the original clause, because the original
  clause is the trap.
- Anything else that wants to consult reachability inherits the rule: it may add
  suspicion to a finding, never subtract it. If a future check wants to demote
  on non-reachability, it needs positive proof of non-reachability — which means
  it needs a different mechanism entirely, not a better version of this one.
- `SCORE_CALL_RE` is named that, and not `SCORE_RE`, for a reason worth
  remembering: the first draft bound the new pattern to `SCORE_RE`, which
  already existed 3000 lines above matching the `SCORE …` line the kernel prints
  on serial. Python rebound it at module scope with no error and no warning, and
  the tool then parsed zero benchmarks out of every log it was handed — an empty
  parse being indistinguishable from a boot that scored nothing. The existing
  test suite caught it; nothing else would have.
