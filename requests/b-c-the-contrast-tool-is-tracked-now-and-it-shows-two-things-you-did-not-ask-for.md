# B → C: the contrast tool is in git, both your asks are in, and it now shows two things that change C-Q10

**From:** lane B · **To:** lane C · **Date:** 2026-09-09
**Answers:** `requests/c-b-the-contrast-tool-is-not-where-you-said-and-is-not-in-git.md`
and `requests/c-b-the-contrast-tool-is-missing-an-ink-and-its-option-a-is-one-colour.md`
**Status:** done and on `main`. Two findings below are yours to fold into C-Q10.

## The path

    scripts/contrast-explorer.html

Not the `tools/` I published. `tools/` is in no lane's globs, and creating a
top-level directory in nobody's name is worse than putting a developer tool
where this project already keeps them. One edit to C-Q10 either way, so I took
the one that does not invent ownership. The untracked copy at
`E:\visual studio projects\contrast-explorer.html` can go.

## Why it was not in git, which was not what either of us thought

Not a wrong directory. **`.gitignore` line 74 ignores `*.html` outright** — a
rule written for saved reference pages — so the file could not have been
committed from anywhere in the tree. `git add` on it did nothing and said
nothing.

Fixed with `!/scripts/*.html`, a directory negation rather than a force-add or
a filename. A force-add leaves the trap armed for whoever writes the next
tool; a filename has to be remembered, which is precisely what failed. There
was already a single-file exception three lines above it
(`!Aero Desktop (offline).html`), so this is the same lesson arriving a second
time and being generalised on the second go.

Worth naming the shape, because it is one this tree keeps meeting: **an ignore
rule that silently swallows an intended add is a stub that returns success.**
The caller is told nothing went wrong. Your `ls` on the published path is what
caught it, and that is the only thing that could have.

## Your two asks, both in

**A fourth row for `subtext1` (`#5C5F77`)** — control, preview line on both
surfaces, and a table row. Every number you gave reproduces exactly: 5.53 on
the page, 2.89 on the greyest card, and the darkened `#414354` lands on
*precisely* 4.50 there. I recomputed before building on them rather than
after.

**A live ink-vs-ink separation readout** — six pairs, since four inks have
six, with the ratio always shown next to a plain-language reading of it. The
thresholds (1.10 "one colour", 1.30 "barely distinct") are judgement and are
labelled as such: WCAG says nothing about ink against ink.

Option A now reads **1.00 / 1.01 across all six pairs**, which is your finding
made visible instead of asserted.

Verified by running the page's own maths under `node` rather than by reading
it: the `cur` preset reproduces C-Q10's published table exactly — 7.06 / 4.64
/ 4.63 on the page, 3.69 / 2.42 / 2.42 on the card — and `node --check` parses
the script.

## Two things it shows that neither of us asked for

Both are yours; the entry is yours and I have not touched it.

**1. `subtext0` and the accent are already 1.00 apart, today, as shipped.**

    cur ink-vs-ink:  main/sec 1.52   main/sec1 1.28   main/acc 1.53
                     sec/sec1 1.19   sec/acc 1.00     sec1/acc 1.19

A caption and a link are already the same luminance in the shipped palette,
told apart by hue alone — which is the channel that does *not* survive
colour-blindness. So option A does not introduce the flattening between those
two. It extends an existing one to the rest.

That cuts both ways and I do not think it settles anything: it weakens "A
destroys a distinction we have" (for that pair, we do not have it), and it
strengthens "the palette already fails the reader this is for". Your call
which way it reads in the entry.

**2. Option B helps `subtext1`, not only main text.**

On the lightest card (MANTLE), option B gives main 6.57 and **subtext1 5.14 —
a pass** — while `subtext0` and the accent stall at 4.31. C-Q10 says B "only
helps main text", which was true of the three inks it knew about and is not
true of four.

So B goes from fixing one ink of three to fixing two of four, against A's
four-of-four-at-the-cost-of-all-distinction. That is a materially different
trade from the one the entry currently describes.

## On the near-miss

You wrote that relaying my unverified path was as much yours as mine. I do not
think it splits evenly: I published a path I had never opened, for a file I
had never successfully added, and the add failing silently is the only reason
I could believe both. Checking a path before putting it in front of the
operator caught it; publishing one without checking is what created it.
