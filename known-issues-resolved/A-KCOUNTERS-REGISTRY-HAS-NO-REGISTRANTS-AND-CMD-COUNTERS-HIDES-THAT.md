## `A-KCOUNTERS-REGISTRY-HAS-NO-REGISTRANTS-AND-CMD-COUNTERS-HIDES-THAT` (lane A, 2026-08-26) -- **fixed 2026-08-29**

**In short:** `kernel/src/kcounters.rs` has two halves — a registry that
subsystems are meant to add counters to, and a hardcoded aggregator that reads
existing atomics directly. The registry half has **zero registrants**: nothing
in the tree invokes `define_counter!`, so `snapshot()` returns an empty vector
on every call and `count()` returns 0, permanently. The `counters` shell command
concatenates that empty list onto the aggregator's real one, so it prints a
healthy-looking table and nothing ever reveals that half the module contributes
nothing.

**The dead surface**, all in `kernel/src/kcounters.rs`, all with no callers
anywhere in the repository:

| item | line | state |
|---|---|---|
| `define_counter!` | 161 | never invoked; only the doc examples at 22-23 |
| `counter_inc!` | 176 | never invoked; only the doc example at 26 |
| `counter_add!` | 184 | never invoked; only the doc example at 29 |
| `CounterDesc` | 50 | constructed only by `define_counter!` |
| `register()` | 93 | `pub unsafe fn`, no callers |
| `seal()` | 105 | no callers, so `REGISTRATION_DONE` is never set |
| `Registry` / `MAX_COUNTERS` (64) | 47-75 | array of 64 `None`, never written |

**Why it is worse than plain dead code.** `kshell.rs:124919` (`cmd_counters`)
does:

```rust
let builtin = crate::kcounters::builtin_snapshot();   // real values
let registered = crate::kcounters::snapshot();        // always empty
let all = builtin.iter().chain(registered.iter()) ... // looks complete
```

`builtin_snapshot()` supplies genuine data, so the command's output is correct
and useful — which is exactly what stops anyone noticing. The empty half is
masked by the live half. Compare the tick-wiring gate's lesson: state that is
never advanced shows a plausible zero rather than an obvious absence, and a
plausible zero is not reportable by any test that only asks "did it print
something sensible?". The command's own doc comment leads with the dead half
("Shows all registered counters ... plus built-in counters"), which is the
reverse of what it actually does.

Note `register()` is an `unsafe fn` with a `# Safety` contract about
single-threaded boot ordering that has never had a caller — so the contract has
never been exercised or reviewed against a real call site.

**The tree has already chosen, in writing.** `builtin_snapshot()`'s own doc
comment (line ~197) says it pulls from subsystem atomics *"rather than requiring
every subsystem to use our macro"*. So the bypass was a deliberate decision; what
was not done is removing the mechanism it bypassed. That is what makes this tech
debt rather than an open design question.

**The proper fix** — delete the registry half, keeping the aggregator:

1. Remove `CounterDesc`, `Registry`, `MAX_COUNTERS`, `REGISTRY`,
   `REGISTRATION_DONE`, `register()`, `seal()`, `snapshot()`, `count()`, and the
   three macros. This also removes two `static mut` accesses and one `unsafe fn`
   from the kernel, which is a security-surface reduction, not just tidying.
2. Simplify `cmd_counters` to read `builtin_snapshot()` alone and fix its doc
   comment to describe what it does.
3. Rewrite the module `//!` doc, whose entire usage example (lines 22-29) is of
   the deleted API.

Keeping `counter_inc!`/`counter_add!` is tempting since they are harmless
one-liners, but they are `fetch_add(1, Relaxed)` spelled longer, and leaving two
macros behind as the residue of a deleted subsystem invites someone to reach for
the registry that no longer exists.

**If instead the registry should live**, the fix is the opposite and larger:
migrate the ~20 hardcoded pulls in `builtin_snapshot()` onto `define_counter!`
and call `seal()` from boot. That is a real option — a registry scales to
subsystems this file does not know about, whereas the aggregator must be edited
for every new counter — but nobody has wanted it in the time the module has
existed, and an unused generalisation that costs a `static mut` is not free.

**How it was found.** Looking for real counters to project into `/proc`-style
files (the same search that produced the `netdev` and `irqstat` entries above);
`kcounters` looked like a rich source until the registry turned out to be empty.

**Not a regression.** True since the module was written — it has never had a
registrant.

**Fixed 2026-08-29** by taking the first option: the registry half is gone.
Deleted `CounterDesc`, `Registry`, `MAX_COUNTERS`, `REGISTRY`,
`REGISTRATION_DONE`, `register()`, `seal()`, `snapshot()`, `count()` and the
three macros -- with them a `static mut` and an `unsafe fn` whose boot-ordering
`# Safety` contract no call had ever exercised. `cmd_counters` now reads
`builtin_snapshot()` alone, its doc comment describes what it does rather than
leading with the dead half, and its empty-case message says "No counters
available" instead of "No counters registered", which named a path that no
longer exists. The module `//!` doc, whose entire usage example was of the
deleted API, now records why there is deliberately no registration path and what
to do instead when a new counter is needed (add the atomic where the event
happens, expose it via that subsystem's stats accessor, add a row to
`builtin_snapshot`) -- so the reasoning survives the code it is about.
