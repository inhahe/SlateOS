### [B] TD-OILS-A-PROCESS-SUBSTITUTION-A-SECOND-SCAN-FINDS-IN-A-BRACE-BODY-IS-NOT-PARSED-AGAIN. bash's `brace_gobbler` and its `${x@P}` re-read each meet a `<(` osh's do not — 2026-08-14 — ✅ FIXED 2026-08-14 (both halves, and the arithmetic-fragment residue)

Two residues of TD-OILS-A-PROCESS-SUBSTITUTION-IN-A-BRACE-BODY-IS-NEVER-PERFORMED
(above), left after both halves of it were done. Each is a *second* scan of the
same text — one that is not `parse_matched_pair` and not `expand_word_internal` —
which has a `<(` row of its own that osh's counterpart lacks. The `$(` spelling
of each already matches bash byte for byte, so in both the machinery is there
and only the row is missing.

**Where:** `userspace/oils/src/interp.rs`, [`Shell::gobbled_subs`]; and the
`${x@P}` re-read, `userspace/oils/src/parser.rs`, [`dquote_word_from_source`].

* **✅ FIXED 2026-08-14.** `echo "${z:-"<(fi)"}"` — bash reports
  `command substitution: line N+1: syntax error near unexpected token 'fi'`
  plus the tail of the physical line, where osh prints `<(fi)`. The agent is
  **`brace_gobbler`**, whose command-substitution row names all three spellings
  (`(c == '$' || c == '<' || c == '>') && text[i+1] == '('`, braces.c:675) and
  reaches `extract_command_subst` → `xparse_dolparen`, which *parses* the body
  and throws the result away. Two facts pin it down. The gobbler's `quoted`
  state does not nest and `${` opens none of its own (it is treated like `\{`),
  so the **inner** `"` is `c == quoted` and clears the state — which is why the
  row fires here and not in the plain `"${z:-<(fi)}"`, where parse.y has
  already answered. And it fires only where brace expansion runs: an argument
  or command word errors (`: "${z:-"<(fi)"}"`, `f "${z:-"<(fi)"}"`,
  `echo "${a["<(fi)"]}"`), while an assignment RHS — which is not brace-expanded
  — does not (`x="${z:-"<(fi)"}"` is silent). bash only ever *parses* it: with a
  body that does parse, `echo "${z:-"<(echo hi)"}"` prints `<(echo hi)` in both
  shells, so this is a diagnostic and not a missing expansion.

  What was missing was something to hang the row on. [`Shell::gobbled_subs`]
  walks the *parse* structurally, and here the tree is right to hold characters
  — the `<(` sits in a `" … "` run inside a double-quoted operand, where neither
  bash's expander nor osh's reads one — so no part was ever going to appear for
  it. The fix is therefore not another lexer mode but a text-level pass beside
  the structural walk, as `gobbled_backtick_subs` already is for a backquote:

  * `wordscan::gobbler_procsubs(s, dquoted)` — the same flat-state loop as
    `gobbler_readable`, reporting the index of each `<(`/`>(` met while `quoted`
    is 0. (`gobbler_readable` could not answer this: it reports the stretches the
    **`$(`** row fires in, which is `quoted == 0` *and* `quoted == '"'`, and the
    `<(` row is the first of those alone.) A `$( … )` is skipped whole rather
    than reported — that is the one spelling a part already stands for.
  * `Shell::gobbled_procsubs` — for each index, lex `$(` + the rest of the word
    with `parser::dquote_word_from_source` and take the resulting
    `CmdSubBody::Unread`. The two spellings reach the same
    `extract_command_subst`, so the swap is exact, and one lex settles the body,
    the remainder and whether there was a `)` at all. It is a *lex*, not the
    paren count `gobbler_readable` skips with, because `xparse_dolparen` is a
    real parse: a `(` inside a quoted run of the body is not a nesting level to
    it, and a count would carve `echo <(echo "(")` into a body that fails.
  * The two are merged by **remainder length**: every tail the gobbler's word
    carries is measured against the whole word (`unparse::gobbler_word`), so a
    longer one is an earlier meeting. That is what keeps the interleaving right
    where a word holds both — measured, `echo "${z:-'$(fi)'"<(for)"}"` reports
    the `$(fi)` and `echo "${z:-"<(fi)"'$(for)'}"` the `<(fi)`.
  * `Shell::has_gobbled_sub` — the cheap pre-test — gained a `WordPart::Literal`
    row, answering wide (any `<(`/`>(` in a literal under quotes) so the word
    reaches the scan that settles it.

  **Verified:** `userspace/oils/tests/corpus/a-process-substitution-a-brace-scan-meets-is-read-where-the-quoting-is-clear.sh`,
  29 rows, all matching bash 5.2.37 — including the parity (`"${z:-"a"<(fi)"b"}"`
  is a *parse* error, `"${z:-"${y:-"<(fi)"}"}"` is silent), the `set +B` gate, the
  words brace expansion does not reach (assignment RHS, `case` word, here-doc
  body), the read happening before expansion (`z=Z`, `${z:+…}`), and the `declare
  -f` re-print.
* **✅ FIXED 2026-08-14 for the double-quoted operand** (`${z:-…}`, `${z:+…}`,
  `${z:=…}`, `${z:?…}` and the plain `${z-…}` family) — which is the position
  the report named, and the only one a `${x@P}`/`PS4` re-read reaches with the
  quoting bash's own expansion declines a process substitution under. The
  remaining positions are a residue of their own, logged at the end of this
  bullet. Original report: `x='${z:-<(fi)}'; echo "${x@P}"` — bash's `extract_dollar_brace_string`
  (subst.c:1881-1950) has a `<(` row of its own and recurses into it with a real
  parse, so the `@P` re-read is a `bad substitution` and the text is printed
  unchanged; osh splices the re-print and prints `<(fi)`.

  **Measured against bash 5.2.37 (2026-08-14).** The row behaves as the `$(`
  row beside it in every respect: `A${z:-<(fi)}TAIL` and `A${z:-$(fi)}TAIL`
  give byte-identical output, down to the quoted remainder `` `fi)}TAIL' ``
  and the `line 2` numbering `xparse_dolparen` gives an unread body. It is the
  scan's row and not the string's — `x='a<(fi)b'` is silent — and it is reached
  only where the scan's own quoting allows: `"<(fi)"` (double-quoted),
  `'<(fi)'` (single-quoted, `skip_single_quoted`) and `\<(fi)` are all silent
  and print their text. A body that parses is silent too and is *not*
  performed: `A${z:-<(echo A >&2)}B` prints `A<(echo A >&2)B` and no `A` on
  stderr.

  osh already matched on six of those shapes. What it got wrong:

  | written (as `x`, then `echo "${x@P}"`) | bash | osh (before) |
  |---|---|---|
  | `A${z:-<(fi)}TAIL` | reports, `bad substitution`, text | `A<(fi)TAIL` |
  | `A${z:-${y:-<(fi)}}B` | reports (nested body too) | `A<(fi)B` |
  | `A${z:-p<(fi)q$(for)r}B` | reports the **`<(fi)`** | reports the `$(for)` |
  | `A${z:-<(fi}B` | `unexpected EOF`, `bad substitution`, text | runs `fi}` — `command not found` |

  All but the last now match. The last is a *different* defect that the `$(`
  spelling has identically — see
  TD-OILS-AN-UNCLOSED-SUBSTITUTION-IN-AN-UNREAD-BRACE-BODY-IS-RUN-INSTEAD-OF-REFUSED
  below — so it was left alone here rather than fixed twice.

  **Why it was not a two-line change.** The `<(` span *is* already collected —
  `Lexer::read_dollar_brace` has the row (lexer.rs:7069) and records a
  `CmdSubSpan` with `SubOpen::Proc`, its `src`, its `range` and
  `SubBody::Unread`. What is missing is a [`WordPart`] for
  [`Shell::brace_scanned_subs`] to walk to: `procsub_reprints`
  (parser.rs:6288) splices a re-print only for a `SubBody::Eager` span, and the
  re-lex that carves the operand out of the body (`read_word_verbatim`) leaves
  a `<(` as characters on purpose. So for an *unread* body the process
  substitution survives only as text in a `WordPart::Literal`.
  `arith_unread_subs` is the shape of the answer for the arithmetic scan, and
  it excludes this spelling deliberately (parser.rs:6233-6240).

  Two things make the obvious fixes wrong, both measured above:

  * **The remainder runs past the `}`.** `` `fi)}TAIL' `` and
    `` `fi)}B${y:-<(for)}C' `` are the rest of the *whole re-read string*, not
    of the `${ … }`. So a text scan confined to the brace's own source (the
    only text [`Shell::brace_extent_scan`] is handed) cannot build the part's
    `tail`, and the `$( … )` spelling gets its own from
    `unparse::attach_comsub_tails`, which runs over the assembled word in the
    parser.
  * **It must interleave with the `$(` spelling**, in the order the one scan
    meets them — hence the `p<(fi)q$(for)r` row above.

  Reusing [`CmdSubBody::Unread`] for the synthesized part is safe for the
  *read* (the diagnostic quotes the body's remnant, never the delimiter, so a
  `<(` and a `$(` in this position are byte-identical) but not for anything
  that re-prints or *runs* one — `interp.rs:34302` performs an unread body, and
  a process substitution here is never performed. So either the part carries
  its spelling (a new field on `CmdSubBody::Unread`, two construction sites and
  one printer, plus the run site taught to refuse) or it is synthesized late
  enough that it can never escape into a print or a run — which is what
  `Shell::gobbled_procsubs` does for the `brace_gobbler` half above, and the
  reason that one could be done without touching the AST.

  **What was done.** The first of the two: the part carries its spelling, which
  makes both blockers vanish rather than needing to be worked around.

  * `ast::SubDelim { Dollar, ProcIn, ProcOut }`, with `bytes()` (the delimiter
    as written) and `is_performed()` (true only for `Dollar`). Recorded on
    `CmdSubBody::Unread` and on the lexer's `SubBody::Unread`. Only the unread
    form needs it: a body a parser *read* is a `CmdSubBody::Parsed` for `$(`
    and a `WordPart::ProcSub` for the other two, so those two shapes already
    tell the spellings apart.
  * `Lexer::read_word_verbatim` gained a `<(`/`>(` row for `Verbatim::Dquote`
    **when the text is unread** (`self.here_text`), emitting
    `Seg::CmdSub(body, close, SubBody::Unread { delim })`. The existing
    `Verbatim::Bare | Verbatim::Replacement` row above it is untouched — those
    fragments really do *perform* the substitution, measured:
    `x='A${z/p/<(echo hi)}B'; echo "${x@P}"` prints a `/dev/fd/N` in bash.
  * `unparse.rs` prints the body back in `delim.bytes()`, and
    `Shell::command_sub_body` returns that text instead of running anything
    when `!delim.is_performed()`.
  * The backslash arm of the same loop takes a `\<(`/`\>(` into the literal
    run, because the *scan* that produced this text honours a backslash
    whatever follows it (`extract_dollar_brace_string`'s `case '\\'`,
    subst.c:1899) while the operand's own dquote read does not. `A${z:-\<(fi)}B`
    prints `A\<(fi)B` and reports nothing.

  Both blockers then answer themselves: the `tail` is filled by
  `unparse::attach_comsub_tails` over the whole assembled word (so it runs past
  the `}`, giving `` `fi)}TAIL' ``), and the interleaving is
  `Shell::brace_scanned_subs`'s existing left-to-right walk.

  **Verified:** `userspace/oils/tests/corpus/a-process-substitution-a-brace-re-read-meets-is-read-like-the-dollar-spelling.sh`,
  22 rows, all matching bash 5.2.37 — the byte-identity with the `$(` spelling,
  both interleavings, the nested body, the not-performed rows (including
  `>(cat)` and a body writing to stderr, quoted and unquoted), the read
  happening before the operand is chosen (`z=Z`, `${z:+…}`), the four shields
  (unbraced text, `" … "`, `' … '`, backslash), the stepped-over subscript, and
  the `PS4` spelling of the same re-read.

* **✅ FIXED 2026-08-14** (every position but the arithmetic one; that one
  closed later the same day, at the end of this bullet)**.** The row
  was wired for the double-quoted **operand** only, and bash's scan reads the
  whole `${ … }` body — it walks characters and knows nothing of the `#`, `/`
  or `^^` it has already passed — so every other fragment wanted the same row:

  | written (as `x`, then `echo "${x@P}"`) | bash | osh before |
  |---|---|---|
  | `A${z#<(fi)}B` (pattern) | reports ×2, `bad substitution`, text | right text, **no diagnostics** |
  | `A${z/p/<(fi)}B` (replacement) | reports ×2, `bad substitution`, text | right text, **no diagnostics** |
  | `A${z^^<(fi)}B` (case pattern) | reports ×2, `bad substitution`, text | right text, **no diagnostics** |
  | `A${z:0:<(fi)}B` (offset) | reports ×2, `bad substitution`, text | `AB` |

  The `$( … )` spelling was right in all four (measured), so again only the row
  was missing. It was harder than the operand's, because in these positions the
  substitution is *both* read for its extent **and** performed — a replacement
  really does expand to `/dev/fd/N`, measured — so the part could not simply be
  the non-performed `CmdSubBody::Unread` the operand's is.

  **What was done.** The split `CmdSubBody` already makes between a body a
  parser read and one only a scan read is now made for the process-substitution
  part too, so one part answers for both halves:

  * `ast::ProcSubBody` — `Parsed(Program)` or `Unread { src, tail, closed }` —
    replaces the bare `Program` in `WordPart::ProcSub`.
  * `lexer::ProcRead` (`Eager` / `Unread { closed }`) rides on `Seg::ProcSub`;
    the `Verbatim::Bare | Verbatim::Replacement` arm of
    `Lexer::read_word_verbatim` picks it from `self.here_text`, and now
    tolerates a missing `)` exactly as the `$(` spelling does.
  * `parser::seg_to_part` parses only an eager body. An unread one is carried
    as text, because its read belongs to the scan and happens later, from
    where a failure is `bad substitution` rather than a script syntax error.
  * `unparse`: an unread body prints back as written, and joins
    `attach_comsub_tails` so it gets the same remainder the `$(` spelling does.
  * `interp`: `Shell::brace_scanned_subs_slice` collects it,
    `Shell::extent_read_of_subs` reads it through the same
    `comsub_reparse_read`, and the new `Shell::proc_sub_body` parses-then-
    performs at expansion — only reachable if that read succeeded.

  **Verified:** corpus case
  `a-process-substitution-a-brace-re-read-meets-is-read-wherever-in-the-braces-it-sits.sh`,
  21 rows, IDENTICAL against bash 5.2.37.

  **✅ The arithmetic fragment, 2026-08-14.** Deferred at first, because osh
  diverged over `<` in a bound before any process substitution was written at
  all (`${z:1<(2)}` is `bcdef` in bash and was an `operand expected` in osh);
  that was fixed as
  TD-OILS-A-LESS-THAN-IN-A-BRACE-ARITHMETIC-FRAGMENT-LOSES-ITS-LEFT-OPERAND,
  and this row followed.

  It was **not** simply `Verbatim::Arith`'s row, as the deferral assumed. A
  subscript shares that mode and must *not* get it: bash's scan steps over a
  subscript whole (`skip_matched_pair` from the `[`), so `${z[<(fi)]}` never
  offers its body to `extract_command_subst` and is an `operand expected` —
  which osh already matched. A bound is walked in the open. So the mode split
  in two: `Verbatim::Bound` / `Frag::Bound`, reached by `lex_bound_verbatim`
  and `parser::word_bound_from_source_at`, identical to `Arith` in every
  respect but that it takes `Dquote`'s unread-`<(` arm. That is the whole
  change — the arm was already written for the operand, and the read/perform
  split it produces (`SubBody::Unread`) is exactly a bound's: read for its
  extent by the scan, never performed, because `Q_DOUBLE_QUOTES|Q_ARITH` is
  what stops `expand_word_internal` (subst.c:11079).

  No interp-side work was needed: `unparse::nested_parts` already classifies
  `ParamSubstr`/`ArraySlice` bounds as `Nested::Operand`, so
  `Shell::brace_scanned_subs_slice` was already descending into them.

  **Verified:** 14 further rows in the same corpus case (the bound in offset
  and length position, `${a[@]:…}` and `${@:…}`, the `@P` and `PS4` spellings,
  the three quotings that shield it, and the well-formed `${z:<(echo 1)}` that
  reaches the evaluator as characters), IDENTICAL against bash 5.2.37.

**How it was found:** implementing the entry above.
