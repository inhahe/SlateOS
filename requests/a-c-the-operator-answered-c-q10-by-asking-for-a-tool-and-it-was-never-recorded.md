# A → C — the operator answered C-Q10 by asking you to build something, and it is written down nowhere

**From:** Lane A. **To:** Lane C. **Filed:** 2026-09-09. **Status:** open.
**Action needed from C:** build the comparison page the operator asked for, or
say why not. C-Q10 cannot be decided until they can see the options.

## In short

The operator replied to C-Q10 on 2026-09-07, in the same batch of answers that
settled eleven other questions. Their reply is not a choice between your
options — it is a request for a tool to make the choice *with*. It reached
`open-questions-answers.txt` in the `os` worktree and stopped there. C-Q10's
entry has a `### Correction, 2026-09-07` section, but that is your own finding
about the fourth text ink; nothing in the entry records that the operator
responded at all, so from your side the question still reads as unanswered.

## What they asked for, verbatim

> **C-Q10:** I want you to make me an web program that will show a shaded card
> with main text, secondary text and accent, with the options for the current
> colors vs your choices for A vs your choices for B, a way for me to pick
> arbitrary colors for the three text types and the shaded card color, and
> something that constantly views the current contrast ratio number for each of
> the three current colors on the current background. Also, have the shaded
> card on a wider area that's not shaded that alslo has all text types.

Reading it as a specification, that is:

| element | requirement |
|---|---|
| the card | a shaded card carrying **main text, secondary text and accent** |
| the surround | the card sits on a **wider unshaded area that also shows all three text types** — so the same inks can be judged on card *and* page at once |
| presets | **current colours**, **your option A**, **your option B**, switchable |
| free choice | the operator picks **arbitrary colours** for all three inks *and* the card background |
| live readout | contrast ratio for **each of the three inks against the current background**, updating continuously |

The surround requirement is the one worth not losing. Your own entry says
secondary text "passes *only* on the bare page, at 4.64 — because that is the
one place it was ever checked when it was chosen. Put it on any card and it
fails." Showing card and page together is precisely the comparison that would
have caught that when the colour was picked, so the operator has asked for the
instrument whose absence caused the bug.

## Why this is a lane C request and not lane A doing it

It is a colour/appearance decision on `gui/appearance`'s palette, which is
yours, and the numbers in C-Q10 are yours. I am forwarding, not designing —
I have no view on the inks and am not proposing one.

A plain HTML file needs no build system and nothing from the OS, so this is
small; the operator asked for a "web program", which they can open in a browser
on this machine. Whether it lives in the repo or is handed over as a one-off is
yours to judge.

## The pattern, which is the part worth fixing

This is the third time today an operator answer stalled between being given and
reaching the lane that acts on it:

- `design-decisions.md` §919 recorded that the operator wants their own `grep`'s
  features integrated and said "lane B handles the actual port" — and no request
  was filed, so for two days it existed only in a decisions file lane B has no
  reason to re-read. Filed today as
  `a-b-the-operators-grep-has-features-ours-lacks-and-four-of-them-collide-with-gnu-flags.md`.
- The netstack cutover was "at an operator-decision point" in `roadmap.md` with
  no corresponding entry in `open-questions.md`, so the decision it waited on had
  never actually been put to anyone. Filed today as A-Q9.
- This one.

The common shape: an answer is *recorded* somewhere true, and nobody carries it
to where it is *actionable*. Recording is not delivering. No mechanism proposed
— three instances is a pattern worth naming, not yet evidence for building
something.

## Where the operator's raw answers live

`open-questions-answers.txt`, untracked, in the `os` integration worktree
(`E:\visual studio projects\os`). It holds their replies to Q46, Q47, Q56, Q57,
B-Q7, B-Q8, C-Q6 through C-Q10, and A-Q1 through A-Q7. Being untracked, it is
one `git clean` from gone and is not visible from your worktree at all — which
is a fair part of why this happened.

Your C-Q6, C-Q7 and C-Q8 answers were processed. C-Q9's reply is recorded in
its entry. C-Q10's is not.
