## §357 — "Printable" is a one-line rule, not a copy of glibc's Unicode table

**Date:** 2026-08-21
**Decided by:** Claude (autonomous) — extending §101/§104's existing rule by two
code points, and measuring the result against glibc rather than asserting it.

**In short:** When a coreutils program mentions a file name in an error message,
it hides characters that would garble the terminal — a raw newline could forge a
whole fake line of output. It has to decide which characters are safe to show.
GNU asks the C library, which answers from a big table of every character
Unicode has ever assigned; that table changes with each Unicode release, so the
same file name can print differently after a system update. We answer with a
one-line rule instead — show anything that is not a *control* character and not
one of the two characters that mean "line break" — which never changes. Checked
against the C library's table over all 1.1 million characters, the two answers
are **identical for every character that exists**; they differ only on code
points Unicode has not assigned to anything, where no font can draw a shape
anyway.

**Related:** §101/§104 (osh's `%q` uses the same rule, minus the two-code-point
extension), §351 (diagnostics quote with the curly marks), §356 (`printf`'s
`\u`), Q38 (osh's string layer is UTF-8, full stop). This is that premise a
fourth time.

### The problem this closed

`quote.rs` escaped *bytes*, with the predicate

```rust
const fn printable(b: u8) -> bool { b >= 0x20 && b < 0x7f }
```

so every byte of a multi-byte character was unprintable by construction. A file
named `café` was reported as `‘caf\303\251’`. That is
`BUG-QUOTE-ESCAPES-VALID-MULTI-BYTE-CHARACTERS`, and it was not a small
cosmetic wart: a user whose language is not English saw every diagnostic about
every one of their files rendered in octal. The fix is to escape *characters*,
which forces the question this section answers — which characters?

### The rule

A character prints as itself unless it is

* a Unicode **control** (`Cc`): the C0 range, DEL, and the C1 range
  `U+0080..=U+009F`; or
* one of the two **line/paragraph separators**, `U+2028` and `U+2029`.

A sequence of bytes that decodes to no character is never printable and is
escaped byte by byte, which is what keeps the answer defined for the arbitrary
bytes a SlateOS path may hold.

### Why not just copy `iswprint`

GNU asks glibc's `iswprint`. Under `C.UTF-8` that is 709 ranges generated from
the `UnicodeData.txt` of whichever glibc release you happen to have. Copying it
would mean:

* **carrying a table that drifts.** Unicode assigns thousands of code points per
  release. A name that printed as itself last year prints in octal this year, or
  the reverse, for no reason the user did anything about — and our table and the
  host's would drift *apart*, so the same name would render differently
  depending on which one you asked.
* **contradicting §101 for no gain.** §104 already declined to model a libc
  printability table for osh's `%q`, on the grounds that "there is no single
  table to copy". Adding one here would leave the two halves of the same
  userland answering the same question from different sources.

### The claim that makes the one-liner safe, and how it is checked

The reason the short rule is not a compromise is empirical:

    $ wsl -e python3 scripts/printable-audit.py
    unicodedata 15.0.0
    824718 code points diverge:
       824718  Cn (ours-only prints it)  e.g. U+0378, U+0379, U+0380, U+0381

Every one of the 1,112,064 code points was compared against glibc 2.39's own
`iswprint`. The divergence is **exactly** the unassigned ones — not "mostly",
not "except for a handful": no assigned character diverges at all. An
unassigned code point is one no font can draw and no standard has given a
meaning, so neither answer to it is wrong, and ours is the one that needs no
table.

That is a claim which could rot, so it is not left as prose. `scripts/`
`printable-audit.py` exits non-zero if a *non*-`Cn` code point ever diverges,
and both fixture tests (`tests/quotearg.rs`, `tests/c_maybe.rs`) carry an
`EXPECTED_DIVERGENCE` list that fails **in both directions**: a listed character
that no fixture row exercises is a claim with no evidence behind it, and one
whose rows have started *agreeing* with GNU is a recorded reason that has gone
stale. A note that has quietly stopped being true is worse than a difference,
because it trains the next reader to trust it.

### Why the two separators are added, and why osh keeps them

`Zl`/`Zp` is a set of exactly two characters, so naming them costs nothing and
needs no table. glibc happens to escape them too, but that is not the reason:
a terminal, a log reader and most `2>&1 | grep` pipelines treat U+2028 as ending
a line, which is precisely the line-forgery this module exists to prevent —
arriving in a character that is not `Cc`. Escaping them would be right even if
glibc printed them.

This makes **coreutils stricter than osh**, whose `needs_ansi_c_quote` (§101)
leaves U+2028 raw, and the two are allowed to differ because they are quoting
for different readers:

| | osh `%q` / `@Q` | coreutils `quote()` |
|---|---|---|
| Quoting for | re-execution by a shell | a human reading a line-oriented stream |
| A separator is | an ordinary character the shell will pass through | a line break that can forge output |
| So U+2028 is | left raw | escaped |

### Alternatives weighed

| Option | Why not |
|---|---|
| Copy glibc's 709 ranges | A table that drifts with Unicode, disagrees with the host's copy, and contradicts §104. |
| Call the host's `iswprint` | There is no host: this is the OS. SlateOS's own libc would need the table anyway, so this only moves the problem. |
| Printable = "not `Cc`", with no separator exception | Leaves the line-forgery hole open in the one module whose whole purpose is closing it. |
| Printable = "not `C*` and not `Z*`" (all controls, formats, separators) | Escapes ordinary text: a plain space is `Zs`, and `Cf` covers the bidi and joining marks that Arabic, Hebrew and Devanagari names legitimately contain. Measured, glibc prints those. |

### Where it lands

`userspace/coreutils/src/quote.rs` → `printable_char`, and the `Piece`
enum/`pieces` iterator that made every escaping loop character-wise rather than
byte-wise. `first_char` and `escape_unprintable` are exported so `printf.rs`
answers the same question from the same place rather than keeping its own copy.
Fixtures were re-measured under `C.UTF-8` by `scripts/quote-probe.py` (8,719
rows) and `scripts/c-maybe-probe.py` (1,828 rows), both widened with whole
multi-byte characters chosen to straddle every edge of "printable" —
before that, every high byte in either corpus was a lone one, so no row could
tell a character test from a byte test.

**Revisit if:** glibc's table ever diverges from this rule on an *assigned*
character — `scripts/printable-audit.py` is what would report it, and the answer
then would be to look at what moved before changing anything.
