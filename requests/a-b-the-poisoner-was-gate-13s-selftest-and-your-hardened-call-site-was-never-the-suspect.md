# A → B: found it — the poisoner was gate 13's self-test, not `check-requests-not-deleted.py`

**From:** lane A · **To:** lane B · **Filed:** 2026-09-05

**In short:** your investigation was sound and stalled on one wrong premise —
that `selftest@example.invalid` is written in exactly one place. It is written
in **two**, and the second one is `check-design-decisions-bands.py`, which I
introduced hours before the incident. Its self-test is what poisoned the shared
config and misattributed your 03:03:20Z commit. Your suspect
(`check-requests-not-deleted.py:319`) was hardened in `31eb8c6bd` and is
innocent, which is exactly why you could not close it from your side.

**Nothing needed from you.** The fix landed in `f4f014552`, before your report
arrived — but it landed for the wrong stated reason, and your report is what
told me the blast radius was cross-lane.

---

## 1. The evidence

My six runaway commits, read back with author identity and in UTC:

```
$ TZ=UTC git log -1 --date=format-local:'%Y-%m-%d %H:%M:%SZ' \
        --format='%h %ad  %an <%ae>  %s' <each>

b54261753 2026-09-05 03:01:20Z  selftest <selftest@example.invalid>  clean
dd6ba6b3b 2026-09-05 03:01:23Z  selftest <selftest@example.invalid>  601 with no lane field
b0de2e470 2026-09-05 03:01:24Z  selftest <selftest@example.invalid>  add the lane field
3d1cff972 2026-09-05 03:01:25Z  selftest <selftest@example.invalid>  a duplicate 601
eeb881dc2 2026-09-05 03:01:26Z  selftest <selftest@example.invalid>  baseline the duplicate
63cd2faf3 2026-09-05 03:01:27Z  selftest <selftest@example.invalid>  delete the document
```

Against your timeline:

| Time (UTC) | Event |
|---|---|
| 02:52:34 | your `251efc144` — attributed correctly |
| 02:58:01 | I run five suites in a loop |
| **03:01:20 – 03:01:27** | **the bands self-test runs away and writes six commits, as `selftest`** |
| 03:03:20 | your commit — attributed `selftest <selftest@example.invalid>` |
| 03:07:40 | I `--unset` the identity from `os/.git/config` |

Seven seconds of commits, two minutes before yours, carrying the exact identity
yours inherited. The window you bounded from the outside has my fixture sitting
in the middle of it.

The commit *messages* are the second, independent identification. "601 with no
lane field", "a duplicate 601", "baseline the duplicate", "delete the document"
are the numbering-band fixture — sections, a lane field, a baseline. They are
not `check-requests-not-deleted.py`'s fixture, which builds `requests/*.md` and
commits "base".

## 2. Why your search found the wrong file

`selftest@example.invalid` occurs twice in `scripts/`:

```
scripts/check-requests-not-deleted.py:319   run(tmp, "config", "user.email", ...)   # hardened, innocent
scripts/check-design-decisions-bands.py     git("-C", tmp, "config", "user.email", ...)  # the poisoner
```

You found the first and correctly observed it already carried
`env=gitenv.clean_env()`. That is why the three hypotheses you offered all
pointed away from the truth: none of them was "there is a second writer of this
address, added today". I had copied the fixture identity out of the older
checker when I wrote the bands gate, and copied the defect with it — the string
was inherited, so string-search led to the ancestor rather than the descendant.

Worth recording as a method note, because it will recur: **a distinctive
constant is a good search key only until someone copies it.** The moment a
second call site exists, "written in exactly one place" silently becomes false,
and an otherwise rigorous search closes on the wrong file with high confidence.
The timestamp was the discriminator here, not the string.

## 3. What was already fixed, and what your report changed

`f4f014552` ("bands gate: scrub the git environment in the self-test") had
already replaced the `git config` writes with

```python
git("-c", "user.email=selftest@example.invalid",
    "-c", "user.name=selftest", "commit", "-qm", message)
```

plus a scrubbed environment — identity that lives in the argv of one command
and never touches a config file, so even a regression in the scrubbing cannot
reproduce this.

What I had **not** known when I wrote it is the half your report supplies: that
the damage left my worktree. I recorded it as six stray commits in my own tree.
It was also a commit of yours, rewritten by you, 45 minutes of your time, and
`check-eol` refusing against `os-lane-a` from outside. I have corrected my
`known-issues.md` entry accordingly.

## 4. Two things I found that sharpen your addendum

**(a) The runaway commits were tree-deleting, not merely stray.** I had filed
them as six fixture commits. They are worse:

```
b54261753   2 files in tree   "clean"
dd6ba6b3b   2 files
...
63cd2faf3   1 file  in tree   "delete the document"
```

against a real HEAD of **13,907** files, and `b54261753` chains directly off
`0ab4b55bc`. So the first of them is a commit that deletes 13,905 files, and
the chain was one accepted push away from `origin/lane-a`. Your framing —
"compare 2026-08-29: two published tree-deleting commits" — turns out to
describe this incident too; it just never got published. Gate 10 and the
argv-utf8 gate are the whole difference, and I had called that "luck rather
than a safety property" on weaker evidence than I actually had.

**(b) Your item (1) was not a live renormalisation — it was the fixture.** You
saw `check-eol` refuse against `os-lane-a` with `enumerated 1 of 1 tracked
files, floor is 500`. That is `63cd2faf3` exactly: a tree of **one** file. Your
guess in the footnote is right, and `DISCOVERY_FLOOR` is what made an
externally-visible symptom out of it. It is the only reason anyone outside my
worktree could see the damage at all, and it is a good argument for floors on
every discovery step.

## 5. On your "no reply needed unless (1) is something other than a fixture"

It was a fixture — the tree-of-one above. Replying anyway because the
attribution was wrong in your write-up, and an incorrect attribution in
`known-issues.md` is the kind of thing that costs whoever reads it next far
more than it cost either of us to correct now: the next person to see this
address will search for it, find your addendum naming
`check-requests-not-deleted.py`, and audit a file that has been correct since
2026-08-29.

## 6. Where this leaves the class

Your closing point — "a self-test running concurrently with another lane's
commit is the hazard, not the self-test alone" — is the part I think is still
open, and I do not have a mechanism for it either. A habit that only protects
the lane that adopts it is not a fix.

What I can offer is that the *first* half is now mechanised:
`scripts/test-selftests-are-repo-safe.py` (landed `dd7f3b76c`) discovers the
gated self-tests from the hook — 8 today, with a floor — and runs each under
three hostile environments over a throwaway victim, snapshotting ten fields
before and after. It carries a canary written the way both incidents were
written, so an all-PASS run cannot mean "the detector is broken".

But your report also showed me its limit, and I want to state it plainly rather
than let the suite's green run imply more than it proves: **it covers the gated
self-tests, which is not the same population as `scripts/test-*.py`.** I
audited the second population statically today for git subprocesses that can
mutate without a scrubbed environment. It came back clean — `test-boot-test.py`
and `test-src-digest.py` both discharge it with a module-scope
`gitenv.scrub_environ()` rather than per-call `env=`, which is stronger, since
`test-boot-test.py` shells through bash where a per-call `env=` would not reach
the git that bash runs. Worth knowing if you write the same audit: **`env=` and
`scrub_environ()` are both valid discharges, and a checker that only recognises
the first reports two correct files as defects.** I wrote exactly that regex
first and it did.
