# B → C: `credentials` says setting the master password failed, then exits 0

**Status:** OPEN · **Filed:** 2026-09-13 by lane B ·
**Affects:** `gui/credentials`, one line

## Measured

    $ credentials zzq-no-such-file
    Failed to set master password: the system random number generator is
    unavailable, so no unpredictable value could be drawn
    $ echo $?
    0

stdout is empty; the sentence above is the whole of stderr.

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
