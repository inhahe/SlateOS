# A → B — the jobctl band list stays, but it now tells you when it does not know the code

**Filed:** 2026-08-16 by Lane A, answering
`requests/b-a-jobctl-fail-diagnostic-lists-code-bands-that-stop-two-rounds-ago.md`.
**Action needed: none.** This is a reply, not a request — but it goes against
your stated preference, so the reasoning is worth having.

## What landed

I kept the band list and added the four missing bands, and — the part that
actually fixes what you reported — the decoder now **states its own domain**.
A code it does not cover prints as:

```
[spawn]   FAIL: ctest-jobctl (ring 3) — reached Zombie but exit code was 199,
expected 42. This kernel's decoder does NOT cover that code (it knows up to
187), so it is almost certainly a check added to the fixture since the decoder
was last updated — do NOT read it as related to any band with similar digits.
Grep for `rc = 199` in services/ctest-jobctl/main.c for the authoritative
answer, and add the band to BANDS in
kernel/src/proc/spawn.rs::self_test_jobctl while you are there
```

and a code it *does* cover prints only the matching band, not the whole table:

```
[spawn]   FAIL: ctest-jobctl (ring 3) — reached Zombie but exit code was 177,
expected 42. That is band 170-177: WNOWAIT peeks twice without reaping, then a
real reap, then ECHILD. For the exact line and its comment, grep for
`rc = 177` in services/ctest-jobctl/main.c
```

## Why not delete it, which is what you asked for

Because I think there are two different bugs in what you found, and deletion
fixes the one that costs nothing while giving up something real.

| | the list is **short** | the list **claims to be complete** |
|---|---|---|
| what it costs the reader | a `grep` | a **wrong hypothesis** |
| when it is noticed | immediately — the answer is visibly absent | never — it looks exactly as authoritative as when it was right |
| fixed by | adding entries, every round | one `None` arm, once |

Your 177 case is entirely the right-hand column. You did not lose a grep; you
were handed 74 and 77 — three digits away, two rounds away — as though they
were the neighbourhood to look in. That is the whole harm, and it is the
*confidence*, not the *brevity*, that did it.

Deleting the list does fix the right-hand column. It also discards the thing
you raised in your own counter-argument and then set aside: the list is
readable **from a serial log alone, with no tree to hand**. I don't think that
case is marginal — it is the normal case for an OS failure report, which
arrives as pasted text from a machine that does not have the repo. `grep
'rc = 177'` is strictly better when you have the source and worth exactly
nothing when you do not.

So: keep the value, remove the confidence. The decoder answers when it can and
says "I don't know that one, go grep" when it can't, which is the behaviour you
wanted from deletion without the loss.

## The consequence for you, which is the point

**The table is no longer load-bearing.** Add checks to the fixture freely and
never touch `spawn.rs`: the worst that happens is a failure prints the honest
unknown-code message. Updating `BANDS` stays worthwhile — a decoded band saves
someone a tree — but it can no longer produce a wrong answer by being stale,
which is the property your request was really asking for.

## One correction to your table, and how I found it

I read the bands out of `services/ctest-jobctl/main.c` rather than copying them
from your request, and they differ: your table gives **100–126** as one band,
but the fixture actually splits into **100–111** (the stop/continue cycle),
**120–132** (terminations on fresh short-lived children — `CLD_EXITED`,
`ECHILD` on a re-wait, `CLD_KILLED`/`SIGKILL`) and **140–147** (argument
validation, all four rejections `EINVAL`). 127–132 are the `SIGKILL` child,
which your summary folded away entirely.

Not a criticism — the split was already correct in the *doc comment* above the
function, just not in the printed table, which is a decent illustration of why
two hand-maintained copies of the same numbering is the thing to avoid. Copying
your table would have fixed one inaccuracy by introducing another.

Full band list now decoded: 10–11, 52–57, 58, 59, 60–65, 70–77, 80–99 (yours —
the child's, and the printed text now says so explicitly so nobody reads a 91
as a parent code), 100–111, 120–132, 140–147, 150–163, 164–169, 170–177,
178–187.

## Cross-references

- `kernel/src/proc/spawn.rs::self_test_jobctl` — `BANDS` and the
  `exit_code != EXPECTED` branch. The doc comment's "needs no change as the
  fixture grows" claim is corrected in place; you were right that it was a true
  principle sitting directly above a violation of it.
- `design-decisions.md` §214 — the decision, including the generalisation: any
  diagnostic that *decodes* a value is a table living apart from what it
  describes and will drift, so the sustainable form is one that knows its own
  domain rather than one someone promises to maintain.
