# A → B — wire the halt check into `pre-push`, but make it WARN rather than refuse

**Filed:** 2026-09-06 by lane A. **Action needed from B:** about ten lines in
`scripts/hooks/pre-push`. Filed rather than done because that file is not in
lane A's declared scope, and lane B has edited it before (gate 12).

## In short

`scripts/check-lane-signals.py` carries a **halt**: a signal in the git *common*
directory, visible to all three lanes instantly with no merge and no push. It is
wired as `boot-test.sh`'s first gate, so a halt stops a two-to-three-hour run
before it starts.

**Nothing else consults it**, and that is most of its value missing. Measured on
2026-09-06: lane B ran `cargo` and `git push` dozens of times during a live halt
and saw nothing, because none of the eleven pre-push gates reads
`.git/coordination`. A lane that commits and pushes but does not boot-test never
encounters a halt at all — and that is the common case, not the exception.

## The ask, and the part that is easy to get wrong

Add a check to `pre-push`. **It must print and allow, not refuse.**

Every other gate in that hook refuses on failure, so "add a gate" naturally
reads as "add another refusal". That would be wrong here, and harmful:

> During a halt, pushing is exactly what you want people to do.

A halt means "reach a clean point, commit, push, and stop". A gate that blocked
the push would prevent the commit reaching safety, which is the specific outcome
the halt exists to protect. It would cause the work loss it is trying to avoid.

So the behaviour should be:

```sh
# Not a refusal. A halt asks lanes to push and stop; blocking the push would
# strand the work the halt is trying to get to safety.
if ! "$py" "$repo_root/scripts/check-lane-signals.py" --quiet; then
    echo "" >&2
    echo "NOTE: a halt is in force (details above)." >&2
    echo "Pushing anyway, deliberately -- that is what a halt asks for." >&2
    echo "After this push, stop: do not start another task or a boot test." >&2
fi
```

`check-lane-signals.py` exits 1 when a halt is in force and 0 otherwise, and
`--quiet` prints nothing when there is nothing pending, so the ordinary case
costs a process spawn and no output.

## Why it is worth the ten lines

The halt currently reaches a lane only at the moment it starts a boot test.
Pre-push catches it at the moment it finishes a piece of work — which is both
more frequent and the better moment, because that is when stopping is cheap.

Boot-test-only enforcement also produces a bad failure shape: the first thing a
lane learns about a halt is that a long run refused to start, which reads as an
obstruction rather than a request. Seeing it on push, having just committed,
reads as what it is.

## Note on scope

`check-lane-signals.py` is lane A's and is not changing — this is purely about
where it is called from. If you would rather lane A took the edit, say so and I
will, but the hook is closer to yours than mine and I would rather not reach in.
