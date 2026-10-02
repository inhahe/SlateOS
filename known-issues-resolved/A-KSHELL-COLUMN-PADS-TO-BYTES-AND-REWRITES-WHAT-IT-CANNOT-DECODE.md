## `A-KSHELL-COLUMN-PADS-TO-BYTES-AND-REWRITES-WHAT-IT-CANNOT-DECODE` (lane A, 2026-08-25) — ✅ **FIXED** 2026-08-25

**In short:** `column -t` exists to make columns line up. It measured each
field's width by counting **bytes**, so any field holding an accented letter
was counted as wider than it is drawn and the next column started early — the
one command whose entire job is alignment got the alignment wrong on any input
that was not pure ASCII. Separately, it decoded its input as text with
`from_utf8_lossy`, so a file or pipe that was not valid UTF-8 came back out
with every undecodable byte replaced by `EF BF BD`, and a CRLF file came back
with Unix line endings. All three are fixed: widths are console cells, and the
data is never decoded at all.

**Where.** `kernel/src/kshell.rs` — `cmd_column`, `cmd_column_input`,
`column_format`, and the `"column"` arm of `dispatch_with_input`.

**What.** Three defects, one root and two branches of it.

```rust
let text = alloc::string::String::from_utf8_lossy(&data);   // ×2
let lines: Vec<&str> = text.lines().collect();
if field.len() > widths[i] { widths[i] = field.len(); }
```

1. **The visible one — padding measured in bytes.** `field.len()` is a byte
   count; the padding it drives is spaces on a console measured in cells. `é`
   is two bytes and one cell, so a field holding it was padded one short and
   the following column began a cell early. Worked example, the fixture rung 62
   now uses:

   | input row | old `widths[0]` | where column 2 started |
   |---|---|---|
   | `é x` | 2 (bytes of `é`) | cell 3 |
   | `ab y` | 2 | cell 4 |

   Two rows, two different column positions, from the command that is supposed
   to produce one. A CJK ideograph fails the other way — three bytes, two
   cells — so the error is not even consistently in one direction.

2. **`from_utf8_lossy` on both the file and the pipe path.** `column` is a
   *formatter*: it re-spaces its input and hands back every other byte of it.
   A lossy decode therefore rewrites the exact thing it was asked to preserve.
   This is the last live `from_utf8_lossy` in `kshell.rs` and a direct
   violation of `CLAUDE.md`'s rule 7 ("No `from_utf8_lossy` — that's silent
   data corruption").

3. **`str::lines` strips a trailing `\r`.** The same half of the bug `sed`,
   `diff` and `comm` each turned out to hold. A CRLF file run through `column`
   came back with Unix endings, matched or not.

**Why it survived.** Every `column` test in the suite used ASCII text with LF
endings — the one shape in which all three defects are invisible. The identical
sentence appears in the `sed` entry above, which is the point: this was the
fourth command found holding the *same two* bugs behind the same
`from_utf8_lossy(…).lines()` idiom, and it was found by grepping for the idiom
rather than by any test noticing.

### Fixed

`column_format` takes `&[u8]`, uses the shared `split_lines` (the splitter the
`sed` fix introduced and `diff`/`comm` then reused), and writes through
`shell_write_bytes`. `dispatch_with_input` moves `column` out of the
"still text-only" group into the byte-clean one.

| | was | is |
|---|---|---|
| `column -t` on `é x` / `ab y` | second column at cell 3 vs. 4 | both at cell 4 |
| `column -t photo.jpg` | every undecodable byte becomes `EF BF BD` | bytes pass through unchanged |
| `column dos.txt` | `\r\n` becomes `\n` | endings survive |
| `column -t` on `Mr.<NBSP>Smith x` | torn into three columns | two, as GNU gives |
| `column -t -s →` | worked by luck (`str::split` on a `char`) | splits on the character's bytes |

**The width function is the interesting part.** `display_width(&[u8])` is a
deliberate mirror of the console's own byte→cell rule
(`console::putchar_normal` plus its UTF-8 accumulator): a lead byte with
`utf8_seq_len >= 2` consumes that many bytes and draws one codepoint at
`unicode::char_width`; an invalid lead byte or an aborted sequence draws U+FFFD
in one cell, with the aborting byte reconsidered rather than swallowed. It has
to mirror it exactly, because a width that disagrees with what is drawn
produces a table that disagrees with the screen — which is the bug this fixes,
in a new place. The one honest gap is stated in its doc comment rather than
hidden: `\t`, `\r`, `\x08` and `ESC` are cursor *motion*, not glyphs, so they
have no width at all; they count 0, and a field containing one misaligns
whatever the function returns. glibc's `wcwidth` returns −1 for these and
util-linux's `column` skips them, so GNU is in the same position and answers
the same way.

**One deliberate behaviour change, and why it is a fix.** The default field
split was `str::split_whitespace`, whose rule is Unicode `White_Space` and
therefore includes U+00A0 NO-BREAK SPACE — the character whose entire purpose
is *not* to be a break. `Mr.\u{a0}Smith` was silently torn into two columns.
glibc's `iswspace(U+00A0)` is false and util-linux tokenises on `" \t"`, so GNU
keeps it whole. `split_ascii_blanks` now splits on space and tab only, which is
both byte-safe and more GNU-faithful — and has the second benefit of leaving a
CRLF line's trailing `\r` attached to the last field, which is never padded, so
the row comes back out `\r\n`.

**Tested by:** `kshell::self_test` rung 62 — nine assertions covering cell
alignment with a two-byte character, an undecodable byte surviving and counting
as its one drawn cell, CRLF round-tripping in both modes, NBSP not splitting, a
multi-byte `-s`, empty fields between two explicit separators, the file path as
well as the pipe, and the supplied final newline.
