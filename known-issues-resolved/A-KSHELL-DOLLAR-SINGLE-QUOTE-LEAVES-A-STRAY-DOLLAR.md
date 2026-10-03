### A-KSHELL-DOLLAR-SINGLE-QUOTE-LEAVES-A-STRAY-DOLLAR. `echo $'hi'` prints `$hi` — 2026-09-12 — FIXED 2026-09-12 (lane A)

**In short:** the shell understands the `$'...'` spelling well enough not to get
confused by it, but not well enough to actually carry it out. Anything typed that way
comes out with a stray dollar sign on the front and its escape codes unprocessed.

**Where:** `kernel/src/shellquote.rs` — the `scan()` state machine has three contexts
(`Unquoted`, `Single`, `Double`) and no notion of `$'`. A `$` in `Ctx::Unquoted` falls
to the catch-all literal arm; the `'` after it then opens an ordinary single-quoted
region. So `strip_quotes` removes the two quotes and keeps the `$`.

**Measured**, with a host harness built from the real module (lines 1-608 have no
`crate::` dependencies, so the scanner compiles on the host unmodified):

| typed | kshell today | bash 5.2.37 |
|---|---|---|
| `$'hi'` | `$hi` | `hi` |
| `$'a b'` | `$a b` | `a b` |
| `$'x\x41y'` | `$x\x41y` | `xAy` |
| `$'\n'` | `$\n` | a newline |

Controls in the same run behave correctly, so this is specific to the construct and not
a broken harness: `'plain'` -> `plain`, `"dq"` -> `dq`, `a\ b` -> `a b`.

**Why it is a bug and not merely an unimplemented feature.** The 2026-08-24 note in
`TD-KSHELL-LINE-EDITOR-IS-UTF8` says the expander's `$'` arm exists to *copy the
construct through unchanged* and leave quote state alone — which it does correctly, and
which fixed a real quoting-inversion bug. The intent was that decoding would happen
later, after quote removal. **Nothing decodes.** So the construct is recognised, guarded,
passed along — and then silently mangled at the end of the pipeline. A user who types
`echo $'hi'` gets `$hi` with no error.

**Three stale blockers found while confirming this, all of which say the work cannot be
done yet, and all three premises are now false:**

| claim | where | status |
|---|---|---|
| "needs an output path that can carry non-UTF-8 — `shell_write` takes `&str` and the capture buffer is a `String`" | `kshell.rs` doc comment on `interpret_echo_escapes` (~10264) | false: `SHELL_OUTPUT` is `Mutex<Option<Vec<u8>>>`, `capture_command -> Vec<u8>`, `shell_write_bytes` and `console::write_bytes` are the primitives |
| "it cannot happen at all until the word path carries bytes end to end" | `kshell.rs` ~1213, the `$'` arm | false — and the comment sits **inside `expand_vars_bytes_inner(&[u8]) -> Option<Vec<u8>>`**, the very function whose signature disproves it |
| "stages (b)+(c) are now gated on operator decision Q45" | `known-issues.md`, Correction 4 | answered 2026-08-21 as option B (§261); the entry already notes it read as blocked for three days after |

This is the third time in this entry's history that a note saying "blocked" outlived the
block. Stale reasoning outlives stale code because code gets exercised and prose does not.

**Bash ground truth for the fix**, captured from 5.2.37 rather than recalled, because the
awkward edges are where a confident misreading lands:

| escape | result | note |
|---|---|---|
| \n \t \r \a \b \f \v | 0a 09 0d 07 08 0c 0b | one byte each |
| \e, \E | 1b | both spellings |
| \' , \" , \\ | 27, 22, 5c | |
| \x41 | 41 | **1-2** hex digits: \x4 -> 04 |
| \xg, \x | 5c 78 67, 5c 78 | no digits -> literal, backslash kept |
| \101 | 41 | 1-3 **octal** digits, no leading 0 needed |
| \777 | ff | wraps to a byte (511 & 0xFF) |
| \0101 | 08 31 | \0 plus up to 3 more octal, then literal `1` |
| \0 | NUL | |
| \8, \q | 5c 38, 5c 71 | not an escape -> literal, backslash kept |
| \cA | 01 | control-letter |
| \uD | 0d | 1-4 hex, encoded **UTF-8** — so an escape can yield several bytes |

**Design consequence.** `scan()` yields one `Tok` per input byte, and `\u`/`\U` can
produce up to four output bytes, so the iterator needs a small pending-output buffer
rather than a one-in-one-out map. Skipping input bytes is safe for the other callers:
`split_bare_words` and `word_start_at` key only off *bare* whitespace, and a byte inside
`$'...'` is never bare.

**Also needed:** `Ctx` gains a `DollarSingle` variant, so `trailing_context` and
`quote_suffix` need arms — inside `$'...'` a backslash *is* special, unlike `'...'`, so
completion must escape differently there. The compiler will enumerate the sites.

**FIXED the same day.** `shellquote::scan` gained a fourth context,
`Ctx::DollarSingle`. On `$` immediately followed by `'` both bytes are consumed as one
structural token, so neither survives quote removal; inside the region `\` introduces an
escape and `'` closes it. `decode_ansi_c` implements the table above.

*Verification, in three layers, none of which can stand in for the others:*

| layer | asserts | catches |
|---|---|---|
| host harness vs **real bash 5.2.37** | 31 cases, byte for byte | our idea of the rules differing from bash's |
| `shellquote::self_test` §10 | 20 expectations in the shipping kernel | the rules being right and not reaching the built binary |
| host extraction of §10's own table | the 20 Rust literals decode to what the harness produces | a typo in an expectation, **before** spending a 24-minute boot on it |

The third layer is the one worth keeping: a self-test expectation is only as good as the
literal it is written with, and `b"\\x081"` is easy to get wrong and impossible to spot by
reading. Extracting the table from the source and re-deriving it caught nothing this time,
which is the outcome you want from it, and cost seconds rather than a boot cycle.

*Two things found on the way, both filed rather than silently absorbed:*

1. **`scripts/check-shellquote-vs-bash.py` is now green about a scanner that is not the
   one shipping.** It grades bash against a Python *port* of the scanner, guarded by
   `assert_port_matches_rust` — which checks only that `DQ_ESCAPABLE` matches. My change
   does not touch `DQ_ESCAPABLE`, so the guard passes while the port lacks the whole new
   context. The docstring is honest about checking "the escape alphabet"; the gap is that
   the alphabet had been serving as a proxy for "the scanner" and has stopped being one.
   It is lane B's file — its own docstring calls `shellquote.rs` "lane A's file" — so it is
   filed as `requests/a-b-shellquote-port-has-drifted-and-the-guard-cannot-see-it.md`
   with the measured rules, not edited.
2. **`$'\0'` deliberately diverges from bash** — dropped rather than truncating the word.
   `todo.txt`, Judgment Calls.

**Why this matters beyond tidiness:** option B (`design-decisions.md` §261) makes
`$'\xff'` *the* way a user names a non-UTF-8 file, since the source line stays text.
That is the whole user-visible payoff of `TD-KSHELL-LINE-EDITOR-IS-UTF8`, and it rests
entirely on this construct working.
