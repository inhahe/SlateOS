## 708. The push boundary refuses a commit whose author is in an RFC-reserved domain — a denylist that cannot false-positive, not an allowlist of the operator's identity

**Date:** 2026-08-30
**Decided by:** Claude (autonomous), lane B
**Lane:** B

**In short:** On 2026-08-29 a test's throwaway git repository leaked into the
real one and wrote `user.email = selftest@example.invalid` into the config that
all three lanes share. For the next 70 minutes every commit anyone made was
signed by that fake identity — 33 of them, plus two whose content was "the
entire repository, deleted" — and all of it reached GitHub. None of it can be
undone, because rewriting published history needs a force-push and this project
forbids those. So the fix has to be *prevention*, and it now lives at the push
boundary as pre-push gate 10: a commit whose author or committer address is in
a domain the internet standards reserve for testing is refused before it can be
published.

**The decision.** Reject on the **domain being reserved** (RFC 2606's
`.invalid`, `.test`, `.example` and `example.com`/`.net`/`.org`; RFC 6761's
`.localhost`), checking **both** the author and the committer field, over the
commits the push would actually add. Not on the address differing from
`git config user.email`.

**Why a denylist here, when an allowlist is normally stronger.** An allowlist —
"the author must be the configured identity" — is strictly more sensitive and
would also have caught this. It was rejected on false-positive cost, which for
a *gate* is the property that decides whether it survives:

| | reserved-domain denylist | configured-identity allowlist |
|---|---|---|
| catches the 2026-08-29 leak | yes | yes |
| catches an unknown future fake identity | only if it uses a reserved domain | yes |
| fires on a legitimate co-author | never | yes |
| fires on a vendored upstream history | never | yes |
| fires on the operator committing from a second machine | never | yes |

The bottom three rows are the argument. A gate whose false positives are
ordinary events gets bypassed reflexively, and a reflexive bypass is
indistinguishable from no gate — except that it also teaches the habit of
reaching for `--no-verify`, which turns off the other nine. The reserved
domains are reserved *precisely so that they can never belong to anyone*, so
this check has no false positives to be worn down by. That is what makes it
safe to leave on, by default, for all three lanes, with no per-lane scoping.

The cost is honest and worth stating: a leaked fixture that used a *plausible*
address would pass. The mitigation is that a fixture written in this tree uses
`example.invalid` by convention, and the one that actually caused the incident
did.

**Why author *and* committer.** They differ on merges, cherry-picks and
rebases, and in the incident both were poisoned. Checking only the author would
pass a merge commit made under a fixture identity — and merges into `main` are
exactly the commits the other two lanes inherit. The suite pins this: a commit
with a clean author and a poisoned committer is refused.

**Why the regex is anchored at both ends, which is the whole check.** The
leading `@` stops a local part matching (`not-a-test@really.co.uk` is a real
address containing "test"). The trailing `$` stops a *prefix* matching
(`invalid.example.co.uk` and `test.mycompany.com` are domains a real party can
register). Drop either anchor and the gate still compiles, still runs, and
still reports success on every push — while either rejecting real people or
accepting the address it exists to catch.

**Why it is not scoped by `touches()`.** Gates 2-6 are scoped to the subtree
that can introduce their defect, so a lane never pays for a check it cannot
fail. This defect has no subtree: it is metadata, not a path, and it arrives
exactly when something other than the author's intent is writing commits.
Scoping it would exempt the case it exists for.

**Alternatives considered.**

- **Fix only the leak, and log the damage.** This is what had already happened
  — `scripts/gitenv.py` and its three call sites, plus a known-issues entry
  asserting "the count cannot grow." The evidence against it is in the history:
  the *second* tree-deletion (`786139a5a`, `2b7d0f0a3`, on lane-b) happened at
  23:47, twenty-five minutes **after** the 23:22 repair. "We fixed the cause"
  and "it cannot recur" are different claims, and only the second one is worth
  anything at a boundary this expensive to cross back over.
- **A commit-msg or pre-commit hook instead.** Earlier is better in general,
  but not here: a bad *local* commit is recoverable by `git commit --amend
  --reset-author` or a rebase, and the entire cost of this incident came from
  the commits being *published*. Placing the gate where the damage becomes
  irreversible is what matches the failure.
- **`git log --format=%aE` (mailmap-aware) instead of `%ae`.** Rejected: a
  committed `.mailmap` could then rewrite `selftest@example.invalid` to
  something innocuous and hide exactly what this looks for. The raw field is
  the one that is actually published.

**Where it lives.** `scripts/hooks/pre-push` gate 10 (bypass:
`ALLOW_FIXTURE_IDENTITY=1`); behavioural coverage in
`scripts/test-pre-push-identity-gate.py`, which drives the real hook over real
pushes in a scrubbed temp repository — including four addresses that merely
resemble reserved ones. Both regex anchors and the grep itself were
mutation-tested: removing the end anchor fails five assertions, blinding the
grep fails three, and the two sets are disjoint, so the suite tests both
directions rather than one. `scripts/test-pre-push-gates.py`'s
`HEADER_ENTRY_RE` was loosened from three literal spaces to `\s+`, because the
header right-aligns its numbers and `#  10. ` has two.

**How to reverse.** Delete the gate 10 block, its entry in the header list, the
count word, and the suite. Note what must not be lost with it: the argument
above is that the *documented* fix (scrubbing the environment) had already been
applied once and the incident recurred anyway. Reversing this without a
replacement returns the project to that state.
