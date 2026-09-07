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

---

## Addendum — lane B's answers to the three things the operator asked *back*

Not operator words; my analysis.

### C-Q7 — the colour-blindness reasoning is right, and it argues for white

The operator's model is correct. Protanopia and deuteranopia (together ~8 % of
men) confuse **red against green**; the blue channel is intact. Cyan
(blue+green) and magenta (blue+red) each keep a blue component, which is why
both survive as *distinguishable from red and green* — the same reason the
operator gave.

But the stronger point is the one in their first sentence: **a highlight does
not have to carry its meaning in hue at all.** Luminance contrast is read
identically by every form of colour vision, including monochromacy, and by
anyone on a failing panel or in sunlight. White on black is the maximum
available and is hue-free, so it is the most robust choice — and it is what
this scheme's own name ("high contrast") is asking for. Cyan is defensible;
white is unimprovable.

The one real argument against white is aesthetic identity: a user who picks
"green on black" may want the scheme to *look* like a green terminal, and a
white highlight breaks that. Which is exactly why the operator's last sentence
is the important one, and I would treat it as binding regardless of the pick:
**the highlight colour must be user-configurable.** Ship white as the default
for the high-contrast scheme, keep cyan available, and let the setting decide.

### C-Q9 — (A) what is normal, and (B) would a user benefit

**(A) What is normal.** The two languages in the table are both standard, and
they are standard *for different jobs*:

- The backup tool's language is **`.gitignore`/`rsync` style** — path-aware
  (`*` stops at `/`, `**` spans directories) and, in `.gitignore` proper,
  `[a-z]` *is* a character class. So on the disputed fourth row the backup tool
  is the odd one out even within its own tradition: git, rsync, tar and
  `dockerignore` all support classes.
- The search tools' language is **glob/fnmatch style**, where classes are
  likewise standard (POSIX `fnmatch` has had them since forever).

So "no character classes" is normal for *neither* family. It is not a
deliberate dialect; it is a missing feature. That inverts the operator's
proposed direction: making search match backup would mean **removing a feature
both traditions have**, and would be the only place in the system where
`[a-z]` in a pattern means five literal characters.

**(B) Would a user benefit.** Rarely, but the cost of *not* having them is not
symmetric. Character classes are uncommon in hand-written exclude lists — most
are `*.tmp`, `node_modules/`, `build/`. The asymmetry is that when a pattern
containing `[` is written, the two behaviours differ *silently*: `[Tt]humbs.db`
either excludes two filenames or excludes nothing at all, with no error either
way. A user who copies a `.gitignore` into the backup tool — the single most
likely way a pattern with `[` arrives — gets a backup that quietly includes
files they meant to exclude.

**So my recommendation is the opposite of the operator's leaning:** add classes
to the backup matcher rather than remove them from search. It is the smaller
change (one matcher, not two), it moves *toward* both upstream traditions
rather than away from both, and it removes a silent-wrong-answer case instead
of creating a second one. If that is rejected, the fallback is not "remove from
search" but "make backup **reject** a pattern containing an unescaped `[`",
so the silent case becomes a loud one.

### The cipher — the three questions about option C, and the speed answer

**"What does in-tree mean?"** That the code lives inside this repository and is
built as part of it, as opposed to being pulled in from an external package
registry at build time. A ported C implementation would sit in the tree as C
source we compile ourselves.

**"Why does it need auditing?"** Because crypto fails silently. A bug in a
renderer shows up as a wrong pixel; a bug in a cipher produces output that
still decrypts correctly with your own code, still passes a round-trip test,
and is simply readable by someone else. Two classes in particular are invisible
to ordinary testing: (i) a construction error (nonce reuse, a truncated tag, a
counter that wraps) and (ii) a **timing side-channel**, where the code takes
measurably longer depending on the secret — that one is invisible to *every*
functional test by definition, because the outputs are all correct. So the only
defence is someone reading the code with intent, more than once.

**"Why harder to audit in-tree?"** Not because C is unreadable, but because of
who has to read it here. This tree is Rust with a strict unsafe policy; the
reviewer reads Rust constantly and C rarely, so a subtle change lands with less
scrutiny. A port also carries provenance that has to be tracked by hand — which
upstream version, which local patches, what changed when it is re-synced —
where a Rust crate written here is reviewed like every other file in the
repository. That is the whole of the "harder to audit" claim; it is about the
review pipeline, not the language's merits.

**"Whichever is fastest with AES-NI, unless the bottleneck is filesystem
throughput."** With AES-NI (plus `PCLMULQDQ` for GHASH), **AES-256-GCM is
decisively faster** — order 1 cycle/byte, several GB/s per core, typically 2–3×
a portable ChaCha20 in plain Rust. So on that criterion alone, B.

**But the exception the operator wrote applies, and it decides it.** What is
actually being encrypted here is credmanager's vault and saved Wi-Fi passwords:
kilobytes, written once when something changes. At that size neither cipher's
throughput is measurable — the cost is dominated by the syscalls and, above
all, by the password-to-key derivation, which is *deliberately* slow (tens to
hundreds of milliseconds) and dwarfs both ciphers by orders of magnitude. The
speed argument is therefore moot for the stated use, and the decision falls
back to correctness: **A (ChaCha20-Poly1305)**, which is one implementation
rather than a fast path plus a dangerous portable fallback.

The one thing that would flip this back to B: if this cipher is meant to become
the basis for **full-disk or filesystem encryption** later, where multi-GB
throughput is the whole point and AES-NI is the reason it is feasible. If that
is the intent, say so in the decision and take B knowing it is two
implementations.
