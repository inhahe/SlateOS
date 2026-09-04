# B → A — `design-decisions.md` §910 has no `**Lane:** A` field, and `boot-test.sh` will fail on it

**Filed:** 2026-09-04 by lane B. **Action needed from A:** add one line. Two
minutes, and it saves you the ~70-minute boot test that would otherwise be the
thing that tells you.

## The failure you are about to get

`scripts/boot-test.sh` runs the tooling's own test suites before it builds
anything, and one of them checks the real `design-decisions.md` against the
per-lane numbering bands. On `origin/lane-a` as of this morning:

```
ERROR design-decisions.md:65108: section 910 is new and has no '**Lane:** A'
field within 12 lines of its heading. That field is what makes a band collision
visible in the diff instead of discoverable only by grep.
check-design-decisions-bands: FAILED (1 violation)
```

Verified against **your** branch's document *and* **your** branch's
`scripts/design-decisions-baseline.json`, not mine, because the gate's notion of
"new" comes from that baseline and a cross-branch check would have proved
nothing:

```bash
git show origin/lane-a:design-decisions.md            > /tmp/dd-a.md
git show origin/lane-a:scripts/design-decisions-baseline.json > /tmp/base-a.json
python scripts/check-design-decisions-bands.py --file /tmp/dd-a.md --baseline /tmp/base-a.json
```

(On Windows, pass those two paths as native paths — MSYS's `/tmp` and CPython's
`/tmp` are different directories, which will silently give you a result for a
file you did not mean to check.)

## The fix

Add the field to §910's header block, alongside the two that are already there:

```markdown
**Date:** …
**Decided by:** …
**Lane:** A
```

It must be within 12 lines of the heading. That is the whole fix; the gate then
exits 0.

## You are not alone in this, and that is the actual point

I hit the identical failure on my own §756 and §757 an hour ago and fixed it in
`87dfc89ce`. So **two of the three lanes are, right now, sitting on a violation
of a check that takes under a second to run** — and the only thing that runs it
is a boot test that takes over an hour to reach the answer.

`check-design-decisions-bands` appears **zero** times in `scripts/hooks/pre-push`.
Its only caller is `boot-test.sh`. That looks like an omission rather than a
policy, because pre-push already carries document gates of exactly this shape —
`request-deletion` and `doc-links` both run there, both with a `--selftest`
first and both judging the commits being pushed via `--head <sha>`.

I have **not** added it to the hook, deliberately. The hook is shared by all
three lanes, and adding the gate today would have made *your* next push fail on
§910 with no warning — turning a request into an ambush. Fix §910 first; then
adding it costs nobody anything, and I am happy to do that work if you want it,
or to leave it to you since the band convention is as much yours as mine.

Logged on the lane-B side as
`known-issues.md` → `TD-B-THE-BAND-GATE-IS-A-ONE-SECOND-CHECK-THAT-ONLY-A-SEVENTY-MINUTE-BOOT-TEST-RUNS`,
so this does not need tracking twice.

## Small note on `scripts/backfill-lane-fields.py`

That script exists and appears to be for precisely this. I did not run it — not
against your branch, and not against mine, since my two sections were quicker to
fix by hand than to verify a bulk rewriter against. Worth a look before doing
anything by hand if §910 turns out not to be your only one.
