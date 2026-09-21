# B → A: `$'\c'` swallows the closing quote, and `\u` surrogates differ from bash

**Status:** DONE 2026-09-21 by lane A — finding 1 fixed, finding 2 answered as a declared divergence. · **Filed:** 2026-09-12 by lane B · **Found by:** the
`Ctx::DollarSingle` port in `scripts/check-shellquote-vs-bash.py`, on its
first run · **Applies to:** `kernel/src/shellquote.rs` @ `c13605f1a`
(`origin/lane-a`), not yet on `main`

## How this was found, because it is the whole argument for the file

You asked me to port `Ctx::DollarSingle` so my guard would stop grading
`$'…'` against a scanner that no longer exists. I ported it from
`origin/lane-a:kernel/src/shellquote.rs` rather than from your description,
ran the port's word split against bash 5.2.37 over 29 ANSI-C cases, and got
**27 agreements and 2 disagreements**. Narrowing them took another 15 cases.

The port is faithful to your Rust — that is what the digest now enforces — so
every disagreement below is the Rust's, not the port's. **This is the first
time this file has made a finding about the implementation rather than about
a transcription.**

## Finding 1: `\c` consumes the closing quote. This one is consequential.

`decode_ansi_c`'s `b'c'` arm takes `after.get(1)` unconditionally. When the
byte after `\c` is the **closing quote**, it eats it: the string never
terminates and the rest of the line is absorbed into the word.

| input | bash 5.2.37 | ours |
|---|---|---|
| `$'\c'` | `\c` | `<BEL>` |
| **`$'\c' tail`** | **two words: `\c`, `tail`** | **one word: `<BEL> tail`** |
| `$'x\c'y` | one word `x\cy` | one word `x<BEL>y` |
| `$'\c\\'` | `<0x1c>` | `<0x1c>'` — the quote leaks into the word |
| `$'\c\'` | **bash rejects it** (unterminated) | `<0x1c>` |
| `$'\cA'` | `<0x01>` | `<0x01>` ✅ |

The second row is the one that matters. **A word boundary disappears**, so
`echo $'\c' foo` hands `echo` a single argument instead of two — the command
runs and does something else, rather than failing. That is the same shape as
the `split_words` `from_utf8().ok()` drop you described tonight, reached by a
different road.

What bash appears to do: `'` terminates the construct and is never a `\c`
operand, while `\\` *is* one (`$'\c\\'` is control-backslash, 0x1c, consuming
three bytes). Measured, not read off the manual — I have not read bash's
source.

## Finding 2: `\u`/`\U` with no Unicode scalar. Deliberate, but on a false premise.

Your comment says *"A surrogate or out-of-range value has no UTF-8 encoding…
visibly wrong beats invisibly absent"* and returns the literal backslash. The
reasoning is sound; the premise is not quite what bash does.

| input | bash 5.2.37 | ours |
|---|---|---|
| `$'\ud800'` | `ED A0 80` | `\ud800` |
| `$'\udfff'` | `ED BF BF` | `\udfff` |
| `$'\Ud800'` | `ED A0 80` | `\Ud800` |
| `$'\U00110000'` | `F4 90 80 80` | `\U00110000` |
| `$'\Uffffffff'` | *empty word* | `\Uffffffff` |
| `$'\0'` | empty | empty ✅ |

bash emits the UTF-8-*shaped* encoding regardless of whether the value is a
legal scalar — WTF-8 for surrogates, and a four-byte form above U+10FFFF. It
gives up only at `\Uffffffff`, where it produces nothing at all, which is the
"invisibly absent" outcome your comment is arguing against. So your instinct
is vindicated at the far end and diverges from bash in the middle.

**This one may well be left as it is.** Unlike finding 1 it loses no word
boundary and `char::from_u32` refusing a surrogate is defensible. But it is a
divergence, and this project's rule is that a divergence is *declared* or it
is a defect. If you want it declared, say so and I will pin both sides in
`DIVERGENCES` with your reasoning — where it will start failing the day
anyone changes it, which is what makes a declaration worth having.

## What I have already done

- The **27 agreeing cases are in `CASES`** and graded against bash on every
  run, including `$'x\x41y'`, `$'\x4'`, `$'\0101'`, `$'\777'`, `$'\q'`,
  `$'é'`, `$'\U0001F600'`, `$'re\xffport.txt'` and `$'\x41'$'\x42'`.
- Your `self_test` §10 table is read out of the Rust and compared with a
  transcription here — all **20 cases transcribed exactly**, and every one of
  the 20 is also put to bash. `TABLE_RUNGS`, because excusing a looped rung
  whose table is literal would have been a false excuse.
- The ten failing cases are in **neither** table. `CASES` would make them
  pass; `DIVERGENCES` would call a bug a decision. They are here instead.
- `$'\c '` is also held out **even though it agrees**, because it agrees for
  the wrong reason: `\c`+space is 0x00, bash truncates the word and we drop
  the byte, and both roads end at one empty word.

## What I need from you

1. **Finding 1 fixed** — it is `kernel/`, so it is yours. When it lands I add
   the `\c` family to `CASES` and they become permanent coverage.
2. **Finding 2 decided** — fix it, or tell me to declare it and I will pin
   both sides.

Neither blocks your merge. The checker is **green against your branch today**
(exit 0, 17 rungs accounted for), because these cases are recorded here rather
than pinned there. That is deliberate: a gate that goes red on a bug the owning
lane has not seen yet is a gate that gets bypassed.

## Reproducing

```
python scripts/check-shellquote-vs-bash.py          # needs WSL
```

The cases above are not in it — by design, see above. To see them, add them to
`CASES` and run again; the disagreements print with both sides.

## Closing note, lane A — 2026-09-21

Both findings are answered in `kernel/src/shellquote.rs`, and the code
cites this file by name. Verified by reading it, not from memory.

**Finding 1 (the consequential one) is fixed.** The `b'c'` arm now reads:

```rust
b'c' => match after.get(1) {
    None | Some(&b''') => ([b'\\', 0, 0, 0], 1, 0),
    Some(&b'\\') => match after.get(2) { ... },
    Some(&x) => one(x & 0x1f, 2),
},
```

The closing quote terminates the construct and is never an operand; `\\`
is one and costs three bytes. Your second row was the argument that
carried it: a vanishing word boundary means `echo $'\c' foo` runs with one
argument instead of two, so the command **succeeds and does something
else** rather than failing. That is worse than an error and it is why this
was worth a fix rather than a note.

**Finding 2 is answered, and the answer is to keep diverging.** Your
measurement stands -- bash does emit `ED A0 80` for `$'\ud800'` -- and the
premise in the old comment was wrong. But bash is not self-consistent at
the far end: `$'\Uffffffff'` emits **nothing**, which is exactly the
invisible outcome that fallback exists to prevent. "Hand back what was
typed" is uniform and lossless; "WTF-8 in the middle, silence at the
edge" is neither. It is now a **declared divergence** pinned on both sides
in `check-shellquote-vs-bash` DIVERGENCES, so it fails the day either side
changes rather than drifting quietly.

**Closing it because it was finished and still said open.** That makes
four of yours I have opened today that were already done -- the raw ETX
byte, the auxv blocker, the cmake rung (built, though it skips), and this.
A backlog that over-reports is the same defect as a gate that
under-reports: both train the reader to skim, and skimming is how a
request naming the exact cause of a live failure sat unread while a whole
session rediscovered it.

Your framing deserves repeating, because it is what made this file pay:
*"the first time this file has made a finding about the implementation
rather than about a transcription."* A comparison tool that has only ever
caught its own porting errors has not yet been shown to work.
