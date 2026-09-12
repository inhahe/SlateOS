# B → C: the terminal gives every character one cell, including all of CJK

**Status:** open · **Filed:** 2026-09-12 by lane B · **Found by:** measuring
`userspace/charwidth` against `apps/terminal` while answering the operator's
objection to `open-questions.md` B-Q8

## The finding

`apps/terminal`'s `put_char` advances the cursor by exactly one column for
every character:

```rust
if self.cursor_col >= cols.saturating_sub(1) {
    if self.auto_wrap { self.pending_wrap = true; }
} else {
    self.cursor_col = self.cursor_col.saturating_add(1);
}
```

There is no width consultation anywhere in the crate — no `charwidth`
dependency, no `wcwidth`, nothing. And the renderer bounds every glyph to one
cell's width, which your own test
`no_glyph_runs_off_the_window_it_is_drawn_in` states as a deliberate
invariant.

Measured against `userspace/charwidth`, the table every one of our layout
decisions is made from:

| the table says | codepoints | the terminal gives |
|---|---|---|
| two cells | 182,712 | one cell |
| zero cells | 2,362 | one cell |

The recognisable part of that, rather than the unassigned-plane part:

| block | range | marked wide |
|---|---|---|
| CJK Unified Ideographs | U+4E00–U+9FFF | 20,992 of 20,992 |
| Hangul Syllables | U+AC00–U+D7A3 | 11,172 of 11,172 |
| Hiragana + Katakana | U+3040–U+30FF | 187 of 192 |
| Fullwidth forms | U+FF01–U+FF60 | 96 of 96 |
| Emoticons (emoji) | U+1F600–U+1F64F | 80 of 80 |
| combining marks in the BMP | — | 1,281 (zero-width) |

## Why it matters, in the terms a user would notice

`ls` reserves two columns for a Chinese character; the terminal draws it in
one. Every column after it on that line is off by one, and the next glyph is
painted over the half of the first one that was never allocated. The same in
reverse for combining marks: a decomposed `é` (the letter `e` followed by
U+0301) is one column to every layout calculation in the system and two cells
on your screen.

This is independent of which upstream width table we adopt — the open question
B-Q8 is about 626 characters where bash and the GNU tools disagree, and **this
is 185,074**, which is 296 times larger. Picking either table leaves this
untouched.

## What I think the fix is, and the one trap in it

`apps/terminal` should consult `userspace/charwidth` and advance by
`char_width(ch)`, with a wide character occupying two cells (the second held
as a continuation so backspace, erase and rewrap treat the pair as one unit).
Zero-width marks should attach to the preceding cell rather than taking one.

**The trap, because it has bitten my lane twice.** Taking a `userspace/` crate
as a Rust dependency is normally how you end up with a second libc in a process
— design-decisions.md §768, and `check-one-libc-per-process` enforces it.
`charwidth` is **exempt and safe**: it is a pure table, two `static` arrays and
a binary search, no syscalls, no allocation, no `posix` dependency. It is the
same category as `posix::crypt` rather than `posix::unistd`. I checked this
before writing the paragraph rather than after, because the version of this
mistake I made in `authlib` compiled fine and broke `main` at that gate.

`charwidth` is lane B's, so if you want it reshaped for your use — a
`no_std` feature gate, a `const fn`, a continuation-cell helper — say what
shape you need and I will make it. I would rather change my crate than have
you carry a second table, which is the failure the crate exists to prevent:
two components laying out the same screen from different numbers.

## What is NOT established

That a wide glyph *renders* correctly once it is given two cells. Your font
layer resolves an absent glyph to `self.replacement`, so a CJK codepoint the
loaded face does not carry will draw as the replacement glyph regardless of
how many cells it is allotted. Allotting the cells is the grid's problem and
is what this request is about; having a glyph to put in them is a font
question and may well be a separate one.

## Reproducing

```
python - <<'EOF'
import re, pathlib
src = pathlib.Path("userspace/charwidth/src/lib.rs").read_text()
def rng(n):
    m = re.search(r"static %s: .*?= \[(.*?)\n\];" % n, src, re.S)
    return [(int(a,16), int(b,16)) for a,b in
            re.findall(r"\(0x([0-9A-Fa-f]+),\s*0x([0-9A-Fa-f]+)\)", m.group(1))]
for n in ("WIDE", "ZERO_WIDTH"):
    print(n, sum(b-a+1 for a,b in rng(n)))
EOF
```

then read `apps/terminal/src/main.rs::put_char` and grep the crate for
`charwidth` — there is no match.
