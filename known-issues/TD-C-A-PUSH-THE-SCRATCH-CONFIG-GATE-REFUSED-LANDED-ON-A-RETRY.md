## TD-C-A-PUSH-THE-SCRATCH-CONFIG-GATE-REFUSED-LANDED-ON-A-RETRY

**Date:** 2026-09-14. **Lane:** C.
**Where:** `scripts/hooks/pre-push` gate 35, and/or
`scripts/check-scratch-config.py`.

**In short:** a pre-push check refused a push, I ran the same push again to
read its message properly, and the second one went through. The commit it
had refused is on `origin/lane-c`. A ratchet that can be passed by trying
twice is not a ratchet, and the fact is recorded here because I could not
work out the mechanism and would rather leave evidence than a guess.

**What is certain.**

| | |
|---|---|
| The content was genuinely bad | `check-scratch-config.py` run by hand on that tree: *"settings wrote slateos/appearance.yaml"*. The warmth-slider test saved the developer's real `appearance.yaml` |
| The first push was refused | the hook printed the gate's full message, ending *"To push anyway: ALLOW_SCRATCH_CONFIG=1"*, and `git push` exited non-zero |
| `ALLOW_SCRATCH_CONFIG` was never set | no invocation in that session set it |
| The commit reached the remote anyway | `origin/lane-c` was at `5cd55fb6e`, the commit carrying the offending test |
| The gate is not simply broken | on the *next* push, with the test fixed, `scratch-config` appears in the hook's `ran:` list and passes |

**A candidate mechanism, offered as checkable rather than as an answer.**
The gate narrows to the crates a push touches, and the file list comes from

```sh
git diff --name-only $pushed_shas --not --remotes="$remote_name"
```

`--not --remotes` subtracts everything reachable from *any* remote-tracking
ref. So a commit that has reached `origin/main` — by the integration
worktree merging and pushing it — contributes no files to that list on a
later `git push origin lane-c`, the list is empty, and the hook's own
comment says an empty list means skip. The narrowing is deliberate and
well-argued in the hook (105 s against ten), and this would be a hole in it
rather than a mistake in the idea.

That does not fit this instance cleanly — the merge to `main` reported
*"Already up to date"*, so `main` should not have carried the commit — which
is exactly why it is written here as a candidate and not as the cause.

**How to check it, cheaply.** Make a commit the gate refuses, push it to a
lane branch and observe the refusal; merge the same commit to `main` and
push that; then push the lane branch again. If the second lane push skips
the gate, the mechanism is confirmed and the fix is to compute the narrowing
against the *remote branch being pushed to* rather than against all remotes.

**Not urgent, and worth saying why.** The gate's job is to stop a defect
reaching `main`, and the boot test runs the unnarrowed check, so the ratchet
holds where it matters. What is lost is the fast feedback — which is most of
its value, because a gate that catches you at push time is a gate you
believe.
