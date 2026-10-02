## TD-A-A-PUSH-THAT-SAYS-REJECTED-AND-NAMES-YOUR-OWN-SHA-HAS-SUCCEEDED (lane A, 2026-09-12)

**In short:** three times today `git push origin main` printed `[remote rejected]`
and exited 1, and the push had in fact landed. The tell is in the message itself:
the SHA it names as the ref's current value is *the one you were pushing*.

```
! [remote rejected]  main -> main (cannot lock ref 'refs/heads/main':
                      is at 9c7680782b53... but expected ...)
error: failed to push some refs
```

**Why it matters more than the wasted minute.** The natural response to a
rejection is to push again; the response to a second rejection is `--force`, on a
shared branch, which this project forbids outright and which would destroy the
other two lanes' work. The message is one of the few places where doing the
obvious thing twice leads somewhere irreversible.

**What it actually is:** the update is applied and something then re-checks the
lock and finds the ref already moved -- a race with the remote's own bookkeeping
or with a concurrent push. Not a refusal, and no gate is involved: the pre-push
hook has already passed by the time this appears.

**The rule: never read a push's exit status as the answer. Ask the remote.**

```sh
git ls-remote origin refs/heads/main       # did the ref move?
git show origin/main:path/to/file | ...    # does it carry what you sent?
```

Content, not just the SHA, because a ref at the right commit still tells you
nothing if you pushed the wrong commit. This is the same discipline the boot
verdicts needed today: a wrapper's exit status describes the wrapper.

**Not fixed, because there may be nothing here to fix** -- it is plausibly the
remote's behaviour under concurrent pushes from three lanes, and it fails in the
safe direction (reports failure, succeeded). Recorded so the next reader does not
escalate.
