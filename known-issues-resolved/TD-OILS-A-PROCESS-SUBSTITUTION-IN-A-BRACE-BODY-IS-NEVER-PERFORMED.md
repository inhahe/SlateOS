### [B] TD-OILS-A-PROCESS-SUBSTITUTION-IN-A-BRACE-BODY-IS-NEVER-PERFORMED. bash runs `${z:-<(echo hi)}` and substitutes `/dev/fd/63`; osh yielded the nine characters `<(echo hi)` — 2026-08-14 — ✅ FIXED 2026-08-14

**Where it was:** `userspace/oils/src/lexer.rs`, [`Lexer::read_word_verbatim`],
which reads the operand, the pattern and the replacement of a `${ … }` and had
no `<`/`>` arm at all.

bash splits this construct across two files and osh had only one half of it.
**Part (A) — the parse** — is `parse_matched_pair` naming `<(`, `>(` and `$(` in
one breath (parse.y:5028) and sending all three through `parse_comsub`
(parse.y:5042), so a `${ … }` body's scan parses a process substitution where it
meets it, its syntax error is the enclosing unit's, and what survives is the
parse *re-printed*; see
`userspace/oils/tests/corpus/a-process-substitution-in-a-brace-body-is-parsed-where-it-is-met.sh`
and [`parser::procsub_reprints`]. **Part (B) — the performance** — is
`expand_word_internal` *running* it, and was this entry.

**The rule** is bash's quoting flag, not the position. `expand_word_internal`
reads a process substitution only when `if (string[++sindex] != LPAREN ||
(quoted & (Q_HERE_DOCUMENT|Q_DOUBLE_QUOTES)) || (word->flags & W_NOPROCSUB))`
lets it (subst.c:11079), so an **operand** runs one when the expansion is bare
and keeps the characters when it is double-quoted, a **pattern** and a
**replacement** run one either way (both are re-entered without
`Q_DOUBLE_QUOTES`), and a **subscript** or a **substring bound** never does
(`Q_DOUBLE_QUOTES|Q_ARITH`), so its arithmetic error names the characters.

**The fix.** [`Verbatim`] gained an `Arith` mode beside `Bare`, `Replacement`
and `Dquote` — identical to `Bare` in every other respect — and
[`Lexer::read_word_verbatim`] gained a `<`/`>` arm live in `Bare` and
`Replacement` only. On the parser side [`parser::verbatim_word_at`] picks the
lexer entry from a new `Frag` (`Word` or `Arith`), which is what a subscript and
the `' … '` runs inside it now pass. The body the arm reads is already the
*re-print* part (A) spliced in, which is what bash performs too: the token
buffer a `${ … }` scan leaves behind holds the re-print and nothing else.

No new expansion machinery was needed. The double-quoted operand was already
right — the splice puts the re-print into the text and its nested `$( … )` then
expands normally, so `"${z:-<(echo $(echo q))}"` is `<(echo q)` in both shells —
so the whole of part (B) was one liveness decision taken at lex time, which is
where osh decides quoting.

**The pre-existing inconsistency this closed.** The substring bound
(`${z:<(echo hi)}`, via [`parser::parse_slice_bounds`]) *did* perform the procsub
while the subscript beside it did not, so osh's two arithmetic contexts — which
bash expands identically — disagreed. The bound is tokenized rather than read
verbatim, so it has no `Verbatim` mode to set; [`parser::word_from_source`], its
only reader, now turns a `Seg::ProcSub` back into the characters it was read
from. Both contexts are on the same side now.

**Verified:** `a-process-substitution-in-a-brace-body-is-performed-unless-the-expansion-is-quoted.sh`,
27 cases across the five contexts. None of them prints a substitution's path —
bash names it `/dev/fd/N` and osh a temporary file — so each asks a question the
path does not answer: whether the text still begins `<(`, whether it names
something that exists, or what a `cat` of it reads.

**How it was found:** implementing part (A) — the eager parse and re-print of a
process substitution met by a `${ … }` body scan.
