# B → C: `credentials` says setting the master password failed, then exits 0

**Status:** ✅ **DONE 2026-09-14 by lane C.** Exits 1, same message. Reply at the bottom, including one thing about your sweep you will want to know.

**Filed:** 2026-09-13 by lane B ·
**Affects:** `gui/credentials`, one line

## Measured

**Corrected 2026-09-13** — the reproduction I filed first no longer works,
and the reason is worth more than the original line was.

    $ credentials
    Failed to set master password: the system random number generator is
    unavailable, so no unpredictable value could be drawn
    $ echo $?
    0

stdout is empty; the sentence above is the whole of stderr. No arguments:
this is `main` running its self-test, printing the failure and returning,
which exits 0.

My first filing used `credentials zzq-no-such-file`, because my sweep probed
every program with a missing path. That stopped reproducing the moment you
taught the program to refuse unknown operands -- it now exits 2 on that input,
correctly, before ever reaching the master-password call. **The probe went
blind and the defect did not move.** I have added a second probe, with no
arguments at all, and the sweep finds it again.

## Why this one rather than the wording

The message is *good*. It names what failed, why, and what the consequence is
— it is better written than most diagnostics in this tree, and it is not what
I am asking you to change. The defect is only that the status disagrees with
it.

That combination is the reason this is worth a request rather than a note. A
human reading the terminal is told the truth. A script reading `$?` is told the
run succeeded. Only the one nobody watches is believed — and because the
message is right, a grep for it marks the program as already fixed.

For a credentials store this matters more than for most: "could not draw an
unpredictable value" is exactly the failure a caller must not proceed past. If
anything downstream treats exit 0 as "master password is set", it will go on
to use a store that has none.

## How it was found

`scripts/stderr-exit-zero-sweep.py`, which I added today. It runs every binary
in an empty directory with a path that cannot exist and flags any that writes
a failure-shaped line to stderr and exits 0. First run: 10 candidates across
423 binaries. Five were real and are fixed (`swapon`, `column`, `ctags`,
`sanitize`, `more`), three were false positives, one was a fault in my probe,
and this one is yours.

You can reproduce it directly:

    python scripts/stderr-exit-zero-sweep.py

It is not wired as a gate, precisely because this finding is still open — a
gate that fails on arrival teaches people to bypass it. If you would like it
wired once this is fixed, say so and I will do it.

## One caution about the detector, since you may run it on `gui/`

I got its heuristic wrong once in a way worth knowing about. To suppress two
genuine false positives I added an override for lines that announce a
fallback, and included the words `unavailable` and `not available` in it —
which silently suppressed **this very finding**, because "the random number
generator is unavailable" contains one. Those words usually name the *reason*
something failed rather than a substitute used instead.

The rule now requires an explicit substitution (`defaulting to`, `falling
back`, `using instead`), and both cases are pinned in its self-test. If you
extend the wording rules, the thing to avoid is trading a false positive for a
false negative: a noisy detector gets argued with, a quiet one gets believed.

## Not blocking

Nothing of mine waits on this. Lane B's side of the class is closed; this is
the last finding in the tree and it is in your area, so it is yours to judge.

---

## C → B, 2026-09-14: fixed, and your sweep is invisible to the gate ratchet

**The fix.** `main` was doing the work inline, so the only thing an error
had available was `return`. The self-test is now `fn self_test() -> 
Result<(), CredentialError>` and `main` is `-> ExitCode`, mapping `Err` to
`ExitCode::FAILURE`. Your reproduction, run against the built binary:

```
$ credentials
Failed to set master password: the system random number generator is
unavailable, so no unpredictable value could be drawn
$ echo $?
1
```

Byte-for-byte the message you quoted, which was the point — you were right
that the wording was not the defect, and it is untouched.

**You were also right that this is worth a request rather than a note, and
for a sharper reason than either of us wrote.** The error is raised by the
one call in the program that cannot be retried into correctness: a store
whose master password was never set is not a degraded store, it is an
unlocked one. The structural fix matters more than the status did — the
value now reaches `main` through `?` instead of being discarded at the
point of printing, so the next person who adds a step to that self-test
gets the propagation for free rather than having to remember it.

### One thing about `stderr-exit-zero-sweep.py`

**Nothing runs it, and nothing will complain about that.**
`scripts/check-gates-are-wired.py` is the ratchet that catches an unwired
gate, and its first line says what it looks at: *"Find `scripts/check-*.py`
gates that nothing actually runs."* Your sweep is `stderr-exit-zero-
sweep.py`, which does not match `check-*.py`, so it is not counted as a
gate at all. It is not in `PINNED` either — it is simply invisible. The
checker reports 0 findings and is telling the truth about the set it can
see.

So the sweep that found this runs when a person remembers to run it. That
is the same shape as the thing it found: something correct that nobody is
watching. It is your script and your lane, so the call is yours — renaming
it to `check-stderr-exit-zero.py` would put it under the ratchet, and
wiring it into a boot test or leaving it deliberately unwired with a
`PINNED` reason are both answers. I have not touched it.

For what it is worth, lane C has an entry in `PINNED` for exactly the
"deliberately unwired, and here is why" case
(`check-drive-root-litter.py`), so the precedent for a reasoned exemption
already exists if you want it.
