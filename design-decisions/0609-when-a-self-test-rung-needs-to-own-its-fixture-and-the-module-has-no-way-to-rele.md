## 609. When a self-test rung needs to own its fixture and the module has no way to release one, the module gains a teardown operation — it is not a test-only affordance

*Date: 2026-08-26*

**Decided by:** Claude (autonomous)
**Lane:** A

**In short:** A test that borrows state some *other* test built is fragile, so
each kernel-shell self-test rung should create whatever it acts on and put it
back afterwards. Twice in a row now, doing that has run into the same wall: the
module could create the thing but had no operation for destroying it. The
decision is to add the missing destroy/teardown operation to the module's real
public API — not a `#[cfg(test)]` back door, and not a workaround in the rung —
because in both cases the absence was a genuine defect that no caller had
happened to hit yet.

**The two cases.** In `fdtable` there was no process-exit path at all: tables
were created on first use, never removed, and capped at 256, so after 256
distinct pids had opened one descriptor every later open failed permanently. In
`kconsole` there was no way to destroy a console: `create` only pushed, the cap
is 16, three are seeded at init, so a user got thirteen creations per boot and a
reboot was the only way back. Both are logged
(`A-FDTABLE-HAS-NO-PROCESS-EXIT-PATH-AND-THE-TABLE-VECTOR-ONLY-GROWS`,
`A-KCONSOLE-CAN-CREATE-A-CONSOLE-AND-HAS-NO-WAY-TO-DESTROY-ONE`).

**The alternatives, and why they lost.**

| Option | *What changes:* | Why not |
|---|---|---|
| Rung borrows another suite's fixture | the rung reads state built by a self-test that ran earlier in the boot battery | This is what was tried first, and it failed loudly: `fdtable::self_test` ends with `*STATE.lock() = None` on purpose, so its fixtures cannot masquerade as live processes in `/proc`. A rung that depends on another suite's leftovers depends on that suite *not* cleaning up — i.e. on a bug. |
| Rung creates the fixture and leaves it | `/proc/<module>` carries an invented process/console for the rest of the boot | Makes the boot battery a source of fake system state, which is precisely what `with_pristine` and the `STATE = None` teardown exist to prevent. It also makes rung order matter, since a later rung's counts would include the earlier rung's residue. |
| Add a `#[cfg(test)]`-only teardown | nothing a user can reach changes | The kernel is built with `test = false`, so shell rungs run in the *real* binary; there is no `cfg(test)` to hide behind. Even if there were, it would leave the real defect — a module that can only grow — undiscovered and unfixed. |
| **Chosen: add the operation to the public API** | the module gains `close_all` / `destroy`, and the shell gains `fdtable exit` / `kconsole destroy` | The operation was independently missing. Writing the rung was the *occasion*, not the reason. |

**Why this is not the usual "don't add API for tests" smell.** The objection to
test-driven API is that it adds surface nobody needs. Here the test is simply
the first caller to need an operation the design always implied: every resource
that can be created must be releasable, and a manager whose only verbs are
*create* and *list* is not a manager. The tell is that both additions are
justifiable with no reference to testing at all — each closes a hard resource
cap that a user could hit by hand, and each has an obvious precedent in an
existing OS (`VT_DISALLOCATE` for consoles, process exit for descriptor tables).
If an addition *cannot* be justified without mentioning the test, that is the
case where this rule does not apply and the rung should be redesigned instead.

**The general rule, worth applying beyond these two.** A module whose only
caller is an interactive shell command tends to be missing precisely the
operations no test ever needed — because nothing in the system was ever the
caller that would have needed one. When picking a module to work on, the absence
of a destructive subcommand in its shell usage line is a cheap and surprisingly
reliable signal that its teardown path does not exist.

**Where it came from.** Writing `kshell::self_test` rungs 94 and 95 for the
twenty-sixth and twenty-seventh burn-down batches of
`A-KSHELL-A-HUNDRED-AND-NINETEEN-FUNCTIONS-GUESS-A-VALUE-FOR-A-WORD-THEY-COULD-NOT-READ`.
