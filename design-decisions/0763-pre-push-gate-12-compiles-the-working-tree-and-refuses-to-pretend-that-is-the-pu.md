## 763. Pre-push gate 12 compiles the working tree, and refuses to pretend that is the push

**Lane:** B
**Date:** 2026-09-04
**Decided by:** Claude (autonomous)

**In short:** A "gate" here is a check that runs when you `git push` and can
stop the push. Eleven of them read the *commits being sent*. The twelfth has to
run the Rust compiler, and a compiler reads files sitting in a directory — it
cannot be pointed at a commit. So the directory and the commits could disagree,
and then the gate would be certifying something nobody is publishing. The
decision is what to do about that gap: this gate checks whether the directory
and the push are the same thing, and if they are not it *checks nothing and
says so*, rather than checking the wrong thing quietly.

**Why the gap exists at all.** The gate exists because coreutils' `#[cfg(unix)]`
code — the half that actually ships on SlateOS — is never compiled by any
ordinary build on this Windows machine, so a type error in it can survive
indefinitely with every build green (see
`TD-B-THE-UNIX-HALF-OF-COREUTILS-IS-NEITHER-LINTED-NOR-TESTED-BY-DEFAULT`).
Closing that means running `cargo` at the push boundary. Every other gate here
takes a `--head <sha>` argument and reads that revision out of git; `cargo` has
no such flag, and never will, because its input is a directory tree.

**The options.**

| | *What changes:* |
|---|---|
| **A. Build the working tree and call it the verdict** | Nothing visible — until the day you push a branch you are not standing on, or push with an edit still uncommitted, and the gate reports a pass for a tree the remote will never receive. |
| **B. Refuse the push whenever the tree is not the push** | `git push` starts failing on ordinary days: a half-finished edit in another file blocks publishing a finished one. |
| **C. Check the sha out into a scratch directory and build that** | The gate becomes always-correct, and always slow: a fresh directory means a cold compile every push (minutes, not the warm 2m16s), plus a second multi-gigabyte `target/` that has to be deleted afterwards. |
| **D (chosen). Build the working tree, but only after establishing it *is* the push; otherwise decline by name** | On a normal push (commit, then push) nothing changes. On an abnormal one the gate prints which condition failed and checks nothing, and the hook's tally lists it as skipped. |

**Why D.** Option A is not a hypothetical failure, it is *this file's own
history*: gate 7 read the working tree instead of the commit being pushed, kept
its shape perfectly while doing so, and put two unformatted commits on
`origin/lane-b`. A gate that answers a question about the wrong tree is worse
than no gate, because the tally then reports it as having run. B trades that for
a gate people would learn to bypass by reflex, which is the failure mode the
hook's header warns about in its own words ("a gate that fails on someone else's
file gets bypassed by habit"). C is the only strictly-correct option and is
rejected on price and on the standing rule against leaving a second `target/`
behind — not on effort.

D's four conditions are: exactly one ref being pushed; its sha equal to `HEAD`;
no modified tracked file; no untracked file. Each names itself when it fails,
because "gate 12 skipped" with no reason is precisely the unreadable silence
the tally was added to end — and unlike a path-scope skip, these are conditions
the author can clear with one command.

**What it costs.** The gate is defeatable by leaving a file uncommitted. That is
real and is not mitigated away: it is bounded by being loud (a decline prints
three lines naming the condition), by being visible afterwards (the tally lists
the gate as skipped on that push), and by the boot test remaining the required
check before any merge to `main`. The alternative reading — that a defeatable
gate is worthless — would apply equally to every `ALLOW_*` bypass in the file,
and the file's design is explicitly that a bypass you have to type is better
than a check people route around.

**The generalisable rule.** When a checker cannot be pointed at the revision
being published, do not silently substitute the nearest available tree. Either
make the substitution provably identical first, or decline. The one thing not
available is answering a different question in the same words.
