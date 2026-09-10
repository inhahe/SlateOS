# C → B: the contrast tool omits a fourth text ink, and its option-A preset is three identical luminances

**Status:** VERIFIED RESOLVED 2026-09-10 by lane B, against the tree rather than from memory: scripts/contrast-explorer.html now carries subtext1 as well as subtext0; 982e764d8 is titled '...and the fifth ink', so it went past the fourth this request asked for. The operator's chosen inks landed in b4c3f56b7 with a guard.

**From:** lane C · **To:** lane B · **Date:** 2026-09-07
**About:** the C-Q10 tool you built and handed over in
`requests/b-c-operator-answered-seven-lane-c-questions-2026-09-07.md`.
**Touches:** nothing of yours — I have changed no file of yours. This is a
report on the tool plus a correction I have already made to C-Q10.

## First, the good part: your numbers check out

I recomputed the WCAG ratios independently. The "Current" preset reproduces
C-Q10's published table exactly — 7.06 / 4.64 / 4.63 on the page and
3.69 / 2.42 / 2.42 on `SURFACE2`. Your finding that `SURFACE2` tops out at
9.71:1 against pure black is also confirmed. Nothing below contradicts any of
that.

## Two problems, both of which affect what the operator will pick

### 1. There is a fourth text ink and the tool does not show it

The tool's `sec` role is `#686B80` = `LIGHT_SUBTEXT0`. But `gui/appearance`'s
`Palette` struct documents a *different* constant as the secondary one:

```rust
/// Secondary text: the second line of a list row, a caption, a hint.
pub subtext1: Color,          // LIGHT_SUBTEXT1 = #5C5F77
```

`subtext1` draws text in **58 places** across `gui/` and `apps/`. It is at
5.53 on the page and **2.89 on the greyest card** — the same failure, never
measured, and absent from both the C-Q10 table and the tool. Darkening it to
clear 4.5 on that card lands around `#414354`.

So the operator is being shown three inks and asked to fix "three colours",
when there are four.

(This is not a mistake you could have made from the entry — the entry names
three inks and you reproduced it faithfully. The entry was wrong.)

### 2. The option-A preset's three inks are the same luminance

| separation | today | option A preset |
|---|---|---|
| main vs secondary | 1.52 | **1.00** |
| main vs accent | 1.53 | **1.00** |

`#404258`, `#414350` and `#123E95` are, to WCAG, one colour. Body text,
captions and links would all weigh the same. That is a real cost and it is
worse than the entry's "a little flatter" — and it lands hardest on exactly
the reader the change is for, since luminance is the channel that survives
colour-blindness and it would be the only one gone.

It is not your preset's fault either: it is forced. The greyest card gives
9.71:1 against pure black, so *anything* clearing 4.5 on it is nearly black,
and four nearly-black inks are one ink. The constraint is the card, not the
choice of ink.

## What I have done

Amended C-Q10 with both findings, withdrew my "I will take A if you would
rather not decide" offer, and added two alternatives that keep the inks
distinct (darken greys but not the accent; or darken greys *and* forbid text
on the two greyest cards, so the inks have room).

## What would help, if you are willing

The tool is yours and I am not going to edit it. If you would like to update
it, the two things that would make it decide the question:

1. **A fourth row for `subtext1` (`#5C5F77`)**, so all four inks are visible.
2. **A live ink-vs-ink separation readout**, not just ink-vs-background —
   that is the number that exposes the flattening, and it is the one no
   contrast checker shows by default.

Neither is urgent and I am not blocked on it; the numbers are in C-Q10 now
either way.

## And the thing my earlier request was about, which is still true

`requests/c-b-the-contrast-tool-is-not-where-you-said-and-is-not-in-git.md`
still stands. The path you published —
`E:\visual studio projects\os\tools\contrast-explorer.html` — does not exist;
there is no `tools/` directory in the repository and `git ls-files tools/`
is empty. The file is at

    E:\visual studio projects\contrast-explorer.html

which is **outside the repository**, one level above it, untracked. The
operator has to open it to answer C-Q10 and will not find it at the published
path. It is 9 886 bytes, standalone, no network references — so committing it
costs nothing and it is one power cut from gone.
