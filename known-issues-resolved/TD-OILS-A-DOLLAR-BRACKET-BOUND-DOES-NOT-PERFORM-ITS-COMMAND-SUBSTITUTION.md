### TD-OILS-A-DOLLAR-BRACKET-BOUND-DOES-NOT-PERFORM-ITS-COMMAND-SUBSTITUTION. `$[ 1+$(… ]` reads the `$( … )` as an arithmetic operand token — 2026-08-14

**Where:** `userspace/oils/src/interp.rs` — the evaluation of a
`WordPart::ArithSub { bracket: true, … }` whose expression text holds an
unclosed `$( … )`.

**What is wrong.** `extract_arithmetic_subst` is
`extract_delimited_string (string, sindex, "$[", "[", "]", 0)` (subst.c:1299) —
flags `0`, so **no** `SX_COMMAND` and no nested read. The `$[` therefore closes
at its `]` by plain delimiter counting, and the unclosed `$( … )` inside is met
later, by the *arithmetic expansion* of the bounds text, which performs it under
`Q_DOUBLE_QUOTES|Q_ARITH`: it reports, runs the abandoned extent, and yields
nothing. osh instead hands the raw characters to its arithmetic tokenizer, which
calls them a bad operand.

Measured (`build/pgY.sh` d1/d2):

| word (inside `v='…'`, via `"${v@P}"`) | bash | osh |
|---|---|---|
| `A$[1+$(for⏎x]B` | reports `for`, runs `fo`, `[A1B]` | silent, `[AA$[1+$(for⏎x]B]` |
| `A$[1+$(echo hi⏎x]B` | reports EOF, `[A1B]` | silent, `[AA$[1+$(echo hi⏎x]B]` |

Row d3 — the same body with no `]` at all — is byte-exact in both shells
(silent, undecoded text), because there the `$[` genuinely never closes.

**What the proper fix looks like.** Two things, in order. (1) `$[`'s lex must
close at its `]` by plain delimiter counting, without the nested read — which
means `Lexer::read_opaque_span` needs to know its enclosing close character, so
that the `$((` spelling (SX_COMMAND) and the `$[` one (flags `0`) can part
company. Routing that arm through `Lexer::unread_comsub_stop` was tried on
2026-08-14 and reverted: it made the `$[` bounds text match bash on d1/d2, but
it *regressed* the `$((` spelling in the corpus case
`an-unterminated-construct-in-text-no-parser-read-is-a-runtime-failure`, whose
`$((1+$(echo` row must report the read and stop rather than condemn the `$((`.
A passing case outranks a documented divergence, so that arm keeps its `?`.
(2) The arithmetic evaluator must perform a `$( … )` in its expression text
with the unread-text rule rather than tokenizing it — which is what makes both
rows' values follow.

**Impact.** Wrong value and wrong diagnostic for a deprecated spelling of
arithmetic expansion, in malformed input, reachable only through `@P`/`PS4`/
here-doc text.

---
