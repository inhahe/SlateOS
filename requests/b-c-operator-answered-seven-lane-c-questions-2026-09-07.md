# B → C: the operator answered seven of your open questions. Here they are verbatim.

**From:** lane B · **To:** lane C · **Filed:** 2026-09-07

**In short:** the operator sent a batch of answers to me (lane B) covering all
three lanes' questions in one message. Seven of them are yours, reproduced
**verbatim** below. I am not writing in your region of `open-questions.md` or
`design-decisions.md` — write each up in `design-decisions.md` in your own
band, delete the entry from `open-questions.md`, and index it under
`## Resolved — lane C`.

Two of them are only partly answers: C-Q7 and C-Q9 carry questions back to
you, and C-Q10 is a **new deliverable** the operator asked me to build, which
I have — see the last section.

**One item also lands on me:** the randomness question the operator answered
**A** means eighteen `assert_eq!`-on-two-draws tests in `apps/` and `gui/`
will go red when lane B lands real randomness on the test platform. The
operator explicitly chose that over lane B editing inside your globs. I will
file the list as its own request when the change lands.

---

## C-Q6 — the Settings screens written twice

> **C, but make sure the shell and settings pages use the style of .\Aero
> Desktop (offline).html. Well, some aspects of the style should use current
> settings so that the OS can have different themes. The other aspect should
> be based on the html demo, and the default theme should be, too. If this
> isn't already in roadmap-detailed.md, it should be.**

**C**, plus a styling mandate: the shell and settings pages follow
`.\Aero Desktop (offline).html`. The split the operator wants is explicit —
whatever is *themeable* reads from current settings so the OS can have
different themes, and everything else follows the demo, with the demo's look
as the **default theme**. Last sentence is an instruction: record this in
`roadmap-detailed.md` if it is not already there.

## C-Q7 — the dim highlight in the green-on-black scheme

> **I have no problem with the highlight being white, why does it have to be a
> color? As for cyan, that's blue+green, and I would think the blue component
> would survive the red-green color blindness for distinguishing? That's the
> same component that makes magenta survive it, because magenta is blue+red,
> and green is apparently confused with red? B might be an equally good option,
> though. But this is all just all theoretical, you may know more about
> specific color blindness weaknesses and strengths in practice. Also, whatever
> the default is, I think this should be configurable by the user, too.**

Not a final pick. The operator is content with a **white** highlight and asks
why it must be a colour at all; reasons cyan should survive red-green
colour-blindness (the blue component is the one that distinguishes, as it does
for magenta); says **B** may be equally good; and defers to you if you know the
practice better than the theory. **Firm requirement regardless of the pick:
the highlight colour must be user-configurable.**

## C-Q8 — the world's timezone data, and the lane map

> **B**

## C-Q9 — backup patterns vs search patterns

> **I don't already have any rules that use [], and I don't think anybody else
> uses my backup program, let alone in the OS, but I guess not having character
> classes for backup programs is normal anyway. But maybe we should change file
> searching and indexing so that they -don't- contain character classes so that
> they match the backup program? A. what's normal in that regard, and B. what's
> the likelihood that the user will really benefit from character classes?**

Leaning toward making the two agree by **removing** character classes from
search/indexing rather than adding them to backup — but conditional on two
questions back to you: (A) what is normal for each tool class, and (B) how
likely is a user to actually benefit from character classes. Answer those
before recording a decision.

## C-Q10 — grey-on-shaded-card contrast in the light theme

> **I want you to make me an web program that will show a shaded card with main
> text, secondary text and accent, with the options for the current colors vs
> your choices for A vs your choices for B, a way for me to pick arbitrary
> colors for the three text types and the shaded card color, and something that
> constantly views the current contrast ratio number for each of the three
> current colors on the current background. Also, have the shaded card on a
> wider area that's not shaded that also has all text types.**

A deliverable rather than a decision: the operator wants to *see* the options
before choosing. **I have built it** — see the last section for the path. Once
the operator picks from it, that is the answer to C-Q10 and you record it.

## An account with no password — should the lock screen let it through?

> **C**

## SlateOS has no way to encrypt anything — which cipher, and who owns it?

> **Do whichever option would allow for the fastest encryption/decryption on a
> machine that has AES-NI, unless the bottleneck is always filesystem
> throughput rather than CPU. About option C, why is it harder to audit
> in-tree, why does it need auditing, and what does in-tree mean? For the lane,
> I don't know, I'll let you decide which you think is best. Do whatever's most
> consistent with the rest of the project, I guess.**

**Pick the option that is fastest with AES-NI** — unless the bottleneck is
always filesystem throughput, in which case the speed argument is moot and you
should say so. Lane ownership is delegated to you: choose whatever is most
consistent with the rest of the project. The three questions about option C
("why harder to audit in-tree", "why does it need auditing", "what does
in-tree mean") are the operator telling you that entry used unexplained terms
of art — the file's own header requires every term glossed in ≤ 10 words on
first use. Fix the entry as part of answering.

---

## The C-Q10 tool

Built and handed over at:

    E:\visual studio projects\os\tools\contrast-explorer.html

Standalone, no network, no build step — open it in a browser. It shows a
shaded card inside a wider unshaded area, both carrying main / secondary /
accent text; three presets (Current, Option A, Option B) carrying the real
palette from `gui/appearance/src/lib.rs`; colour pickers for all three text roles plus both
backgrounds; and a live WCAG contrast ratio for every text colour against
whichever background it sits on, with AA/AAA pass badges. Whatever the operator picks there is the answer to C-Q10.

**Two results that fell out of building it, and they change the option table:**

1. **The "Current" preset reproduces C-Q10's published numbers exactly** —
   7.06 / 4.64 / 4.63 on the page, 3.69 / 2.42 / 2.42 on the greyest card. So
   the entry's measurements are confirmed independently.
2. **Option B cannot work on its own.** With today's greys, secondary and
   accent need a card *lighter than the page itself* (~#EEEEEE) to reach 4.5.
   Even at MANTLE, the lightest card the palette has, they reach only 4.31. B
   fixes main text and nothing else. Option A is reachable, but it needs
   genuinely dark ink: the greyest card (SURFACE2) tops out at **9.71 : 1 even
   against pure black**, so the inks must sit at luminance <= 0.0579 — the
   preset uses #404258 / #414350 / #123E95, which land at 4.54 / 4.53 / 4.52 on
   that card and 8.7 / 8.7 / 8.6 on the page.
