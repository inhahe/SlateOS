### TD-OILS-AN-UNMATED-DOUBLE-QUOTE-GROWS-A-MATE-WHEN-THE-WORD-IS-PRINTED-BACK — 2026-08-14 — ✅ FIXED 2026-08-14

**Where:** `userspace/oils/src/unparse.rs` — `part_src`'s
`WordPart::DoubleQuoted` arm, which writes a `"` on both ends unconditionally;
the run that has no closing `"` is built by `userspace/oils/src/lexer.rs`,
`Lexer::read_word_verbatim`'s `'"'` arm under `ParseOpts::tolerant`.

**Repro** (bash 5.2.37, `build/pr11.sh` t1):

```sh
z=ZZ
v='A${z:-'"'"'i"t'"'"'$(fi)}B'; printf '[%s]\n' "${v@P}"
```

| | bash 5.2.37 | osh |
|---|---|---|
| remainder quoted by the read | `` `fi)}B' `` | `` `fi)"}B' `` |
| word named by `bad substitution` | `A${z:-'i"t'$(fi)}B` | `A${z:-'i"t'$(fi)"}B` |
| value | `[A${z:-'i"t'$(fi)}B]` | same |

**What is wrong.** In text no parser read, a `"` with no mate is not an error:
`string_extract_double_quoted` is handed a *finished word* and its walk ends at
the end of the string as readily as at a quote (that is
`ParseOpts::tolerant`, and the corpus case
`a-double-quote-with-no-mate-in-an-operand-runs-to-the-end-of-the-operand.sh`
pins the expansion of it). The resulting `WordPart::DoubleQuoted` therefore
covers a run whose closing quote **was never in the source** — but the part
does not record that, and `part_src` prints the pair back. Every consumer of
`crate::unparse::word_src` then sees one byte that was not in the word.

The value is unaffected, because quote removal drops the `"` either way. What is
affected is everything derived from the *text*: `Shell::bad_sub_word` (the word
`bad substitution` names), the tail `extract_command_subst` quotes back in its
own diagnostic, and — in principle, though no divergence has been measured for
it yet — `crate::wordscan::word_fault`, which re-scans `word_src` for the
unclosed `${`/`` ` `` verdicts and could be pushed either way by a stray quote.

The single-quote analogue exists in the same shape:
`Lexer::read_single_quote` has a `None if self.opts.tolerant => return Ok(s)`
arm, and `part_src`'s `WordPart::SingleQuoted` likewise writes both `'`s. No
divergence has been measured for it, because the paths that produce an unmated
`'` do not currently reach a diagnostic that prints the word back — but the
defect is the same one and a fix should cover both.

**What the proper fix looks like.** Record the missing mate on the part rather
than guessing at print time: `Seg::Dq(Vec<Seg>)` → `Seg::Dq(Vec<Seg>, bool)`
and `WordPart::DoubleQuoted(Vec<WordPart>)` → a `closed` field, exactly as
`Seg::Sq(Str, bool)` already carries its own flag, with `part_src` writing the
trailing quote only when it was there. About 27 sites mention `DoubleQuoted`
across `ast.rs`, `parser.rs`, `interp.rs` and `unparse.rs`; most are matches
that need only a `..`. The single-quote half is the same edit on
`WordPart::SingleQuoted`.

Not worth reaching for a cheaper trick: an unmated run always extends to the
end of its text, so "omit the quote when the part is last" would be *nearly*
right, and nearly-right quoting is how a word stops re-parsing.

**Fixed 2026-08-14**, along the lines above. `read_double_quote_until` now
reports whether a `"` really ended the run — it has exactly two `Ok` returns,
one per case, so the flag falls straight out of the existing control flow — and
that rides on `Seg::Dq(Vec<Seg>, bool)` into
`WordPart::DoubleQuoted { parts, closed }`. `part_src` writes the trailing quote
only when `closed`. The single-quote half is the same edit:
`read_single_quote`'s tolerant arm answers `false`, `Seg::Sq` became a struct
variant `{ text, escaped, closed }` rather than grow a second unnamed `bool`,
and an unmated run prints as `'` + text instead of going through
`sh_single_quote`, whose whole job is to supply the mate.

Two returns needed thought rather than transcription: the pair inside
`read_double_quote_until` that end the run on an *unclosed construct* absorbed
into a `Seg::Unclosed` answer `false`, since the run ended on the construct and
not on a quote; and the backslash spelling of `Seg::Sq` is unconditionally
`closed: true`, having no quotes to match.

Corpus case:
`userspace/oils/tests/corpus/a-double-quote-with-no-mate-does-not-grow-one-when-the-word-is-printed-back.sh`
— 8 shapes including `PS4` and a here-document body, byte-identical to bash
5.2.37 including stderr.

**Impact while it stood.** Diagnostics only — one spurious `"` in the two lines
bash prints for a malformed `${ … }` whose operand holds a `"` opened inside a
`' … '` run. Reachable only through `@P`/`PS4`/here-doc text.
