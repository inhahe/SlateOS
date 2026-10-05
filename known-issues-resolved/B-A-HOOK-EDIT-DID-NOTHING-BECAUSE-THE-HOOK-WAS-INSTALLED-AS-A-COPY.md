## `B-A-HOOK-EDIT-DID-NOTHING-BECAUSE-THE-HOOK-WAS-INSTALLED-AS-A-COPY` (lane B, 2026-08-30) — **FIXED 2026-08-30**

**In short:** Git "hooks" are little scripts git runs automatically at certain
moments — ours run at `git push` and refuse the push if a check fails. The
tracked source of the push hook lives at `scripts/hooks/pre-push`, but what git
actually ran was a **copy** of it made by `scripts/install-hooks.sh` at some
earlier date. So editing the tracked file changed nothing until somebody
remembered to re-run the installer, and nobody did. A gate added, committed and
pushed on 2026-08-30 never ran — including on the very push it was written to
check. Fixed by making the installer install a *trampoline* (a two-line stub
that finds and runs the tracked file) instead of a copy.

### Why it is the worst shape a guard can fail in

**"The gate found nothing" and "the gate does not exist" print the same thing,
which is nothing.** The push output looked entirely normal: every other gate
reported in, the getopt sweep printed its usual `1 table(s) checked; 0
disagreement(s)`, and the new `selftest: 5/5 rules ok` line simply was not
there. Absence of a line is not something a reader notices, and there was no
non-zero status anywhere to notice instead. This is the same rule the gates
already state about their own exemption lists — *a check that cannot fire must
not be indistinguishable from a check that passes* — turned around and pointed
at the delivery mechanism rather than the check.

It was found only because the line was expected within a minute of adding it. A
gate added and not watched for would still be dormant.

### The second failure, which is worse and was silent for longer

`.git/hooks` is **shared by all four worktrees**. `core.hooksPath` is unset, and
a linked worktree has no `hooks` directory of its own, so git falls back to
`$GIT_COMMON_DIR/hooks` — one directory, serving `os`, `os-lane-a`, `os-lane-b`
and `os-lane-c`.

One installed copy cannot serve four checkouts sitting at four different
commits. The hook runs the checker scripts out of `git rev-parse
--show-toplevel` (`pre-push` line 191), which resolves per push to *the pushing
lane's* tree — so a copy installed from lane B's checkout was already running
lane A's checker scripts whenever lane A pushed. **Hook body and checker scripts
came from different commits**, and whichever lane re-ran the installer last
silently set the hook version for the other three. A lane that added a gate and
correctly re-installed it would have been overwritten by the next lane to run
the installer, with no message either way.

### The fix

`scripts/install-hooks.sh` now writes an eight-line trampoline per hook:

```sh
root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
real="$root/scripts/hooks/${0##*/}"
if [ ! -f "$real" ]; then
    echo "hook ${0##*/}: $real does not exist in this checkout; nothing ran" >&2
    exit 0
fi
SLATEOS_HOOK_TRAMPOLINE=1
export SLATEOS_HOOK_TRAMPOLINE
exec sh "$real" "$@"
```

Four properties, each answering one of the failures above:

| | |
|---|---|
| It resolves the tree **at push time** | each lane runs its own checked-out hook, so hook and checkers come from one commit |
| It names the hook by `$0`'s basename | the body has no version of its own to drift, and is the same for every hook |
| `exec` rather than a call | keeps stdin — git feeds `pre-push` its ref list that way, and a trampoline that swallowed it would make every hook see an empty push |
| It sets `SLATEOS_HOOK_TRAMPOLINE=1` | `pre-push` warns on stderr when that is unset, which is exactly the condition "I am a stale copy" |

A missing tracked hook is a **skip with a message on stderr, not a failure** —
checking out a commit from before the hook existed (a bisect, an archaeology
dig) must not make the tree unpushable, and the message says why nothing ran.

Installing it is a one-time act per clone, and one run arms all four worktrees
because they share the one `.git`. It is already done. Verified the same day:
the next push printed `selftest: 5/5 rules ok` followed by the getopt sweep,
which is the line whose absence started this.

### The hook now says what it did, which is the other half of the fix

The trampoline stops the hook being *stale*. It does not stop it being
*absent* — and on a push that edits only documentation every path-gated check
legitimately skips, so `pre-push` printed nothing at all, which is exactly the
output a hook that never ran produces. That ambiguity is what hid this bug, and
removing the cause without removing the ambiguity would leave the next delivery
failure just as invisible.

So each gate now calls `note_gate <name> <skip-flag>`, and the hook ends with an
unconditional tally:

```
pre-push: 3 gate(s) ran, 7 skipped.
  ran:    private-file rustfmt fixture-identity
  skipped: unreachable-command raced-global argv-utf8 getopt-table host-errmsg
           quote-names request-deletion
```

Three readings that silence could not carry: **no line at all** means the hook
did not execute; `0 ran, 10 skipped` means the push was judged by nothing; and a
gate named in `skipped` on a push that plainly should have triggered it is a
`touches` pattern that is too narrow — previously indistinguishable from a gate
that ran and was content. A refusing gate exits before the tally, so it records
what was *asked*, not that the push passed.

Gate 5 is the shape worth copying. Its `skip_getopt` was not its real skip
condition: a push that changed no coreutils bin left both of its blocks as
no-ops while the flag still said "running". Having to report itself forced the
effective condition to be named (`skip_getopt_eff`), which is the usual effect
of making a thing account for what it did.

### Where else to look

- **Any check whose only evidence of running is a line of output.** Prefer a
  gate that reports a *count* it had to compute (`1 table(s) checked`) over one
  that prints only on failure: a count of zero and a check that never ran are
  distinguishable, silence and success are not.
- **Anything else installed by copying a tracked file into an untracked
  location.** The class of bug is "the artefact and its source can disagree, and
  nothing compares them". `git config --get core.hooksPath` should stay unset;
  if a future change sets it, the trampoline is bypassed and this returns.
