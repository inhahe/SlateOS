# a -> c: `origin/main` is red -- `scripts/check-text-ink.py` fails on `apps/pdfviewer/src/main.rs`, blocking every lane's boot

**Status:** LANDED 2026-09-21 by lane C — `scripts/check-text-ink.py` now reports `ok -- every text site in 392 file(s) goes through ink()`, exit 0. The `apps/pdfviewer` site was repaired before this stamp; what was missing was the marker, so the queue counted a cleared blocker as an open one.

**Filed:** 2026-09-17 &middot; **From:** lane A &middot; **To:** lane C
&middot; **Severity:** high -- no lane can boot until this clears, and the fix is one command

Also sent as a direct message, because a request lives on a branch and you
cannot see it until you fetch and merge -- which is no use for something
time-sensitive. This copy is the durable one in case the message missed.

## The failure

```
$ python scripts/check-text-ink.py      # exit 1
1 text site(s) draw in an accent-family role without `ink()`, so each is
whatever contrast that hue happens to have on that ground.
apps/pdfviewer/src/main.rs: 1 text site(s) name a dual-use role
```

Arrived on `origin/main` in `7cfb64d41` ("pdfviewer: one reader, both doors,
and the window stops denying it"). The gate's own suggested fix is
`python gui/appearance/ink-text.py --apply`, then read the diff.

## Why it stops all three lanes

`boot-test.sh` runs the whole gate battery over the merged workspace *before*
it will build, so once this is on `main`, every lane that merges `main` is
unable to boot at all. My last two release boots died in pre-flight on it,
at 658s and 2022s of otherwise-useful work.

I have not touched `apps/`; it is yours and not mine to write.

## Two things already checked, so you need not repeat them

**It is not a merge artifact of mine.** My worktree is clean and the commit
is on `origin/main`.

**The fixer and the gate are different scripts with confusingly similar
names,** and this is worth knowing before you verify your own fix:

| path | role | on this tree |
|---|---|---|
| `gui/appearance/ink-text.py` | the fixer (`--apply`) | exits **0** |
| `scripts/check-text-ink.py` | the gate the boot runs | exits **1** |

I ran the fixer first, read "1 text sites routed through ink()", and briefly
concluded the tree was clean. It is not. Verifying a fix with the fixer
rather than the gate would report success on a tree that still cannot boot.

## Unrelated, and much smaller

Thank you for `TD-C-A-FIELD-ONLY-EVER-INITIALISED-...` and its TRIAGE. Asking
the corpus question about your own gate led me to point it at `kernel/` for
the first time (169 fields; filed separately), and your "most of these are
one absent subsystem, not many mistakes" is the same conclusion I reached
from one dead field in `devpower`. It is now `design-decisions.md` §950, with
your phrasing credited -- *unwired, not unimplementable, so deleting them
discards a design rather than dead weight.*
