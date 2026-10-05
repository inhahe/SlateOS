## `TD-C-THE-LEXER-BLANKED-PRODUCTION-CODE-AND-EVERY-CHECKER-BELIEVED-IT` -- **FIXED 2026-09-21** (lane C)

**Status:** FIXED 2026-09-21

**In short:** the helper that removes test code from a Rust file, before any
of our twenty-six checkers reads it, was deleting live code as well. In one
app it deleted 70% of the program. Every checker pointed at that file was
looking at its type declarations and nothing else, and reporting confidently
on what it found.

**The mechanism.** `scripts/rustlex.py`'s `live_code` blanks each
`#[cfg(test)]` item by matching its braces. To find the item it looked for
the next `{`. But an attribute can sit on a **struct field**, which ends at a
comma and has no braces of its own:

```rust
    #[cfg(test)]
    rng: SeededRng,
}

impl SpeedTestUI {          // <- the next `{` is this one
    pub fn start_test(&mut self) { ... }
```

The brace match therefore consumed the whole `impl`. In
`apps/speedtest/src/main.rs` that is `start_test`, the tick handler, and
every assignment to `phase` -- so `scripts/frozen-flag-survey.py` reported
`phase` as a field nothing writes while eight live assignments sat inside the
blanked region. I only found it because I refused to believe the survey
against what I could read in the file.

**This is the third instance of one assumption**, and `live_code`'s own doc
comment already names the first two: it replaced
`src.split("#[cfg(test)]")[0]` -- "the first attribute is the last thing in
the file" -- and then a `mod` special case, "the first `#[cfg(test)] mod`
is". Now: "the next `{` belongs to this attribute". Each time the repair was
the same: stop inferring the item's extent from a character, and read what
the attribute is actually on.

**Measured, 3,147 files containing `#[cfg(test)]`:**

| | before | after |
|---|---|---|
| live characters | 45,317,463 | 45,422,645 (+105,182) |
| share of non-blank source | 64.6% | 64.7% |
| files gaining >1000 chars | | 6 |
| files losing anything | | 0 |

| file | live before | after |
|---|---|---|
| `apps/speedtest/src/main.rs` | 29% | 56% |
| `userspace/coreutils/src/bin/bc.rs` | 51% | 71% |
| `userspace/m4/src/main.rs` | 47% | 76% |
| `gui/compositor/src/lib.rs` | 47% | 49% |
| `gui/window/src/lib.rs` | 60% | 69% |
| `apps/mandelbrot/src/main.rs` | 46% | 53% |

**The aggregate is the point.** 64.6% to 64.7% is nothing, and that is why
this lasted: the damage was six files deep, not spread thin. `live_code`'s
docstring says it in as many words, about the *previous* instance -- "**a
floor on the total cannot see a hole in the distribution** -- which is worth
remembering before trusting one anywhere else." It was worth remembering
about the very function it is written on.

**A second shape, found by looking for it rather than waiting for it.** An
attribute also lands on a *statement*:
`#[cfg(test)] self.computes.set(..);` in `apps/mandelbrot`. The first version
of the repair stopped at commas and braces, so it ran to the enclosing
block's `}` and blanked every live statement after it -- the same defect, one
shape along. Mandelbrot hid it, because there the statement happens to be the
last in its block; a synthetic case with a statement following it did not.
`_field_end` now knows three terminators: a field or variant ends at its
comma, a statement at its semicolon, and the last of either at the `}` or `)`
that closes the container -- which belongs to the container, so the scan
stops before it.

**Verified across the consumers rather than assumed.** All twenty-six
importers of `rustlex` were run against the fixed lexer; the seven `check-*`
gates all exit 0. Making *more* code visible is exactly what trips a ratchet
written while the code was invisible, so this was checked before the commit
rather than discovered in another lane's build.

**Nine fixtures** now pin it in `rustlex.py`'s own self-test, including the
two regressions the repair could have introduced: a `fn` whose argument list
contains a comma must still be treated as a block, and a last field with no
trailing comma must not eat its struct's closing brace.
