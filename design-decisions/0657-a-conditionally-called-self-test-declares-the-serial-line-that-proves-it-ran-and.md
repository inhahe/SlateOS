## 657. A conditionally-called self-test declares the serial line that proves it ran, and the gate that reads it is a question about history, not about one boot

**Date:** 2026-08-31
**Lane:** A
**Decided by:** Claude (autonomous)

**In short:** Some of the kernel's self-tests are only run when a condition
holds — "test the FAT filesystem, *if* a FAT filesystem got mounted". If the
condition is never true, the test never runs, and nothing says so: the boot log
looks exactly the same as one where the test was never written, and the summary
line above it still says PASSED. Seven tests sat like that for a year. The fix
is to make each such test state, in a comment next to the call, the exact line
it prints when it runs — so a script can check the log instead of asking a
person to.

### The problem with the previous arrangement

`check-self-tests-wired.py` printed, on every run:

> 6 self-test call site(s) in main.rs sit inside a conditional … **Check each
> against the serial log before citing it as coverage.**

The reasoning was right and the output was a homework assignment. It was read
once. A guard that ends in an instruction to a human is a guard that runs on
every boot and is enforced on none of them.

It is also an assignment a human does badly. Doing it by hand on 2026-08-31 I
recorded a **false negative** on two of the six, because I searched the log for
the function's name near the word "self-test" and those two print *integration
test*. The name-to-output mismatch is precisely what a script does not get wrong
and a reader does — which is the argument for keying the check on a declared
literal rather than on anything derived from the symbol.

### Decision 1: the marker is a declaration next to the call, verified printable

`// RAN-IF: "<literal>"` sits in the comment block touching the call site, and
`check-self-tests-wired.py` requires that the literal actually occurs in the
file that *defines* the suite.

The alternative was to infer the marker — take the function's module tag, or
its first `serial_println!`. Rejected: inference is what the false negative
above was. A declaration is checkable, and checked.

The printability check is not a nicety, and it is the part most likely to be
dropped by someone in a hurry. **A marker that matches nothing is worse than no
marker at all.** No marker fails loudly, here, now, in the pre-build gate. A
typo'd marker *passes* that gate and then reports its suite as never-run on
every boot forever — an accusation against working code, which of the two
failure directions is the one that gets believed and acted on. Requiring the
literal to live in the defining file (not merely somewhere in the tree) closes
the sibling case: `Running self-test...` untagged is printed by dozens of
modules, so an untagged marker would report coverage the suite does not have,
and would keep reporting it after the suite stopped running.

Contiguity with the call is required for the same reason — a marker allowed to
sit anywhere in the function survives its call being deleted, and becomes a
permanent false "this ran".

### Decision 2: markers are keyed by literal, so mutually exclusive arms share one

`acpi::self_test` is called from both arms of one `if`/`else`. Two call sites,
one literal.

Keying by *site* was the obvious implementation and is wrong: seeing the line
proves *an* arm ran without saying which, so the losing arm would be reported
as never-seen on every boot — on code that is correct by construction, since one
arm always runs. Keying by literal makes the gate claim exactly as much as the
log can support and no more.

### Decision 3: the gate is pre-build over history, not post-boot over one log

`known-issues.md` had proposed a post-boot check beside the recorder, which is
where the serial log is. That is the wrong place, and the reason is the whole
point of the feature: **a gated suite legitimately not running on one boot is
not a defect — that is what "gated" means.** A post-boot check would either fail
on correct code or assert nothing at all.

The defect is only *never* running, and "never" is a question about history. So
the gate reads `bench/boot-history.jsonl` before the build, exactly like
`check-boot-skips.py`, and asks whether a marker has been absent on 100% of the
boots recording it, N ≥ 10. That does not prove the branch is unreachable; it
proves nobody has ever observed it being taken, which for a test is the same
news.

**Cost accepted:** the gate is blind until ten boots have accumulated. It says
so out loud rather than printing OK, because a permanently-zero count is also
what a broken `--gated-markers` wiring would look like, and those two must never
render identically.

### Decision 4: the denominator is per marker

`gated_ran` records the markers declared *at the time that boot ran*, so a
marker added today is absent from every older row's key set.

Using the window as the denominator — the obvious reading, and what
`check-boot-skips.py` does — would make a newly-declared marker read as
never-run out of twenty-five: the gate failing loudest at the exact moment
someone declares a marker correctly. The `skips` field can use the window
because it is a *complete list* per boot, so absence means "did not fire";
`gated_ran` is a key set that grows, so absence means "not declared then", which
is not evidence of anything. Each marker is therefore measured only against the
rows carrying its key, and the N ≥ 10 floor applies to that count.

The same asymmetry drives two smaller calls: a marker the newest boot no longer
declares is **retired**, not failed (an accusation with no address, whose only
remedy would be allowlisting a line that is not in the tree), and an allowlist
entry is stale only when the tree *contradicts* it — never merely when there is
not yet enough evidence, which would reject every new entry for being new.

### Decision 5: substring, not regex

Every real marker contains `[`; four contain `(` and `)`. Matching them as
patterns would give those characters meaning they do not have, match nothing,
and report all five suites as never-run. The plain `in` test is the correct
reading of a literal, not a shortcut past a proper one.

### Residual risk

The allowlist. `check-boot-skips.py`'s docstring already argues this at length
and the same argument holds here: an allowlist is where a gate goes to die, and
the two properties that stop it are that an allowlisted marker still prints on
every run, and that an entry contradicted by the tree is a failure rather than a
shrug. `ALLOWED` is empty today and the test enforcing that every entry states
an observable condition is already in force, so the rule exists before the first
entry does.

### How to reverse it

Delete `scripts/check-gated-selftests.py` and its `check_gated_selftests` call
in `boot-test.sh` to drop the verdict while keeping the data; additionally drop
`--gated-markers` from `boot-test.sh` and the `RAN-IF` requirement from
`check-self-tests-wired.py` to return to the note. The six `main.rs` comments
are inert on their own.
