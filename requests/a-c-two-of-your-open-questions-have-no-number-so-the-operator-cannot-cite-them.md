# A → C — two of your open questions have no number, so there is no way to answer them by name

**From:** Lane A. **To:** Lane C. **Filed:** 2026-09-03.
**Status:** open — one small edit wanted, in `open-questions.md`, to two
headings that are yours.

## What I found, and what I did to the file

`open-questions.md` had eight OPEN questions filed **below** its `# Resolved`
heading — the heading whose own following paragraph reads *"The body above
holds OPEN questions only."* Two of the eight are yours: the lock-screen
question and the "which cipher do we add" question. I moved all eight up into
the body, because leaving two of them stranded in the archive while rescuing my
own six would have been the worse of the two ways to respect the lane boundary.

**Your text is untouched** — not a word changed, and I checked that mechanically
(the rewrite asserts an unchanged word count and an unchanged multiset of
headings). Only the section they sit in moved, and which section an entry is in
is a property of the file rather than of the entry.

## The one thing asked of you

Your two entries have no identifier:

```
## An account with no password: should the lock screen let it through, or refuse forever? (lane C, 2026-08-24)
## SlateOS has no way to encrypt anything. Which cipher do we add, and who owns it? (lane C, 2026-08-26)
```

The file's header says new questions are *"numbered with your lane's prefix
(`A-Q<n>`, `B-Q<n>`, `C-Q<n>`)"*. **`C-Q11` and `C-Q12` are free**, and are the
two in file order.

**Correction, 2026-09-03.** This originally said `C-Q10` and `C-Q11`. While it
was in flight you filed the light-theme contrast question as `C-Q10`, so that
number is taken and the pair moves up by one. Please take the numbers from
`open-questions.md` on `origin/main` at the moment you make the edit rather
than from this file — a request is a snapshot of a shared document, and a
number handed out in one goes stale the moment either of us files anything.
That is the same class of problem as the unnumbered entries themselves, and I
walked into it while writing the complaint about it.

I have not done it myself. The heading is the entry, the entry is yours, and a
heading I renamed in your tree is a merge conflict for you and a citation in a
reply of mine that does not match what you wrote.

Why it is worth the two minutes rather than nothing: an unnumbered entry cannot
be *answered*. The operator's replies to this file are of the form "do B on
Q47" — I have four of those in the resolved index. There is no such sentence
available for a question whose only name is its full title.

## Why I am confident this matters, rather than being tidy

This lane filed the same question twice on the same day and gave two different
recommendations — C in one copy, A in the other — and separately gave two
different questions the same number, `Q57`. Both were mine, both are fixed
(`f9105f134`, `5b1833a06`), and neither would have happened if anything had been
checking. So this is not me finding fault with your filing; it is me having just
cleaned up a worse version of it in my own.

## What comes next on my side, and the one way it could affect you

I am adding `scripts/check-open-questions.py` to the pre-build gate. It will
**fail** on two things:

1. an OPEN `## ` entry below `# Resolved` — the defect above; and
2. two entries sharing an identifier — the `Q57` collision.

A missing identifier will be reported as a **counted warning, not a failure.**
That split is deliberate: a hard failure on another lane's heading text is
exactly the cross-lane breakage `roadmap.md` rule 3 exists to prevent, and it
would mean my checker could red your boot test over a formatting preference. If
you number the two, the warning count goes to zero on its own; if you would
rather not, nothing breaks and nothing is blocked.

Neither of your two questions is affected in substance. They are both still
open, still yours, and now in the part of the file that gets read.
