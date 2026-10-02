## TD-A-SELFTESTS-REACH-OUTSIDE-THEIR-OWN-MODULE — `with_pristine` restores one module, and a suite is not confined to one module

**In short:** the fix for the destructive self-tests swaps a module's state
for an empty one, runs the suite, and puts the real state back. That covers
the module the wrapper is applied to and nothing else. A suite that calls
into a *neighbour* — clearing the service registry, clearing the event log,
writing files — still destroys what it touches, and now does so while
wearing a wrapper that reads as if it is safe. Three of the 35 eager modules
did exactly that. All three are fixed; the point of this entry is the class,
and the check.

**Lane A. Found 2026-08-23, before wiring the eager suites into the boot
test.** Found by static survey, not by a boot panic — which matters, because
at boot these tables are empty and every one of them would have gone green.

**What was found:**

| Suite | Reached into | Effect on a live machine |
|---|---|---|
| `svcstart` | `fs::servicemgr::clear_all()`, twice | deregisters every service on the machine |
| `logpersist` | `eventlog::clear()` twice, and `flush()` | appends fabricated events to `/var/log/events/combined.jsonl` and can rotate genuine history off the end of it |
| `eventlog` itself | its own ring, via `clear()` at the start of test 1 and again as "clean up" | discards the operator's whole event history |

`eventlog` also carried a dead `let _saved_total = total_events();` — a
save with no matching restore, i.e. someone had noticed this exact problem
and left a stub of the fix.

**Fixed in `d29938b53`** by giving `eventlog` and `fs::servicemgr` a
`pub(crate) fn with_pristine_state<R>(body: impl FnOnce() -> R) -> R` and
composing the wrappers. Exposing a *function* rather than the `static` is
deliberate: both modules keep state outside the table (`GLOBAL_SEQ`, `OPS`)
that has to be made pristine alongside it, and a caller holding the `Mutex`
would have to know that. `logpersist` additionally redirects its log
directory to `/tmp/logpersist-selftest` and removes it afterwards — the
directory is chosen by whoever calls `init`, not by the state the `static`
holds, so `with_pristine` could not have redirected it.

**The check is `build/survey_reach.py`.** For each `self_test_inner` it
reports every `crate::`/`super::` path in the body, plus every `use crate::…`
imported name used bare there (which is not a refinement — `net::bridge`
imports `MacAddress` and then writes `MacAddress::new(..)` with no `crate::`
anywhere). It is a filter, not a verdict: a call that leaves the module
through a helper *in* the module is invisible to it, so a clean report means
"nothing obvious".

**Deliberately not fixed: `net::traceroute`** advances
`icmp::next_trace_seq` and `icmpv6::next_trace6_seq` by two and does not put
them back. The counter is a correlation nonce, it is `wrapping_add`, the
suite registers no probe under the numbers it burns, and the assertions are
relative to what it read. Recorded in the wrapper so the next reader of
`survey_reach.py`'s output does not re-investigate it.

### A fourth instance, 2026-09-11 — and why eighteen days passed without anyone noticing

**`sockact::self_test` was the fourth, and it was worse than the three above.** It
called `crate::fs::servicemgr::clear_all()` at the *start* of its body, seeded the
table with `init_defaults()`, and then called `clear_all()` **again** as its
"Clean up" — so it finished by leaving the machine's service registry empty rather
than as it found it. It also reset its own `STATE` by hand at both ends, so a live
socket-activation configuration was destroyed rather than restored. It ran at every
boot, from `main.rs:6565`.

Latent for precisely the reason this entry already gives: *"at boot these tables are
empty and every one of them would have gone green."* `main.rs` never seeds the
service registry, and `servicemgr`'s own properly-wrapped suite runs *after* this
one. The damage needs a machine with services registered before that point, or a
suite run from the shell — which is to say it needs someone to be using the feature.

Fixed by composing the same two wrappers `svcstart::self_test` uses, whose doc
comment describes this identical hazard in almost the same words about the same
function. The trailing "Clean up" is gone rather than kept: a second wipe that reads
like a restore is worse than no cleanup at all.

### The cause of the eighteen-day gap, which is the part worth keeping

This entry says **"The check is `build/survey_reach.py`."** `build/` is gitignored
(`.gitignore:88`), so that script was never tracked by git, is absent from the tree,
and cannot be recovered from history. Nothing has re-run the survey since
2026-08-23. The class was declared closed, the guard against recurrence was written,
and the guard evaporated on the next clean checkout — leaving an entry that *names*
a check nobody can run.

Replaced by **`scripts/check-selftest-reach.py`**: tracked, ten self-test cases,
wired into `boot-test.sh` ahead of the build, and verified by pointing it at the
pre-fix `sockact.rs` — where it reports all three reaches. It must be static and it
must be pre-build, for this entry's own reason: no boot test can catch a wipe of a
table that was empty to begin with.

Its limits are stated in its docstring rather than discovered later. The verb list
is a filter, not a proof — a destructive function with an unexpected name is
invisible, exactly as the original survey could not see a call that left the module
through a local helper. The wrapper check is per *file*, not per suite. What the
gate adds over the lost script is not cleverness but permanence, plus an allowlist
that forces a sentence about each surviving pair: of the twelve destructive reaches
in the tree, five are moved aside by the reached module's wrapper and seven are
allowed with a reason (`nameservice::init_defaults` early-returns when already
initialised; `restart_block::clear(t1)` names one task the suite created).

### How many other named checkers are gone: 24 of 29

Measured across `known-issues.md` and `known-issues-resolved.md` — every path
matching `build/*.{py,sh}` that the documents cite. Five survive; twenty-four do not.

**That is mostly fine, and the distinction is the useful part.** The missing ones
are overwhelmingly one-off repro and bisect scripts (`pgS.sh`, `pq1.sh`,
`pr11.sh`, `repro-wait.sh`) whose job ended when the bug was understood. Even the
surviving `build/mutation_check.py` is a 75-line one-off — *"third opinion on
survey2.py's quiet bucket, and the one that retires the whole
classify-then-convert approach"* — whose value was a conclusion that now lives in
this file rather than an instrument anyone needs to re-run. `survey_destructive.py`,
`widen_check.py` and `survey2.py` were explicitly superseded by it and are
correctly gone.

So the rule is not "never write a script in `build/`". It is:

> **A one-off measurement may live in `build/`. A standing check may not.**
> The tell is the tense the document uses. "`build/x.py` *established* that…" is
> an argument, and the argument survives in prose. "**The check is** `build/x.py`"
> is a guard, and a guard in a gitignored directory is deleted by the next clean
> checkout while the sentence claiming it goes on reading true.

`survey_reach.py` is the only one of the twenty-four that was cited in the second
form. It is also the only one whose absence cost something.
