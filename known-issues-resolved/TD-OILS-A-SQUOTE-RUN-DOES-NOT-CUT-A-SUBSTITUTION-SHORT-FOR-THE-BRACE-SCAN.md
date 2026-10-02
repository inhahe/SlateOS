### TD-OILS-A-SQUOTE-RUN-DOES-NOT-CUT-A-SUBSTITUTION-SHORT-FOR-THE-BRACE-SCAN. A `$( … )` opened inside one swallows the read that should have followed it — 2026-08-14

**Where:** `userspace/oils/src/lexer.rs` — `Lexer::read_word_verbatim`'s `$`
arm in [`Verbatim::Dquote`], reached through
`Shell::brace_extent_scan` → `Shell::brace_scanned_subs`.

**Repro** (bash 5.2.37, `build/pr12.sh`):

```sh
z=ZZ
v='A${z:-'"'"'p$(echo hi'"'"'q$(fi
S1}B'; printf '[%s]\n' "${v@P}"
```

| | bash 5.2.37 | osh |
|---|---|---|
| reports | ``syntax error near unexpected token `fi' `` | **nothing** |
| value | `[AZZB]` | same |

**What is wrong.** The two passes bash makes over this word carve it into
*different constructs*, not merely read the same constructs differently.

- `extract_dollar_brace_string` meets the `'` and hands the run to
  `skip_single_quoted`, which stops at the **mate**. So `'p$(echo hi'` is one
  skipped run, the `$(` inside it is never seen at all, and the scan resumes at
  `q` — where it meets `$(fi⏎S1}B`, reads it, and reports `fi`.
- `expand_word_internal` has no `'` left to speak of, so its
  `string_extract_double_quoted` meets the **first** `$(`, hands the rest of the
  word to `extract_command_subst`, and — there being no `)` anywhere — takes
  everything. One substitution, not two.

osh derives the brace scan's reads from the expansion's lex, so it gets the
second carving and the second `$(` is inside the first's body, where the walk
never reaches it. `Shell::brace_scanned_subs_slice`'s single-quote bookkeeping
then correctly suppresses the one construct it *can* see (it is inside the run),
and the result is silence.

This is the residue of
`TD-OILS-AN-UNDECODED-BRACE-BODY-IS-RE-LEXED-AS-A-DOUBLE-QUOTED-RUN`, which
fixed the part of the same disagreement that was only about *which openers*
count. Rows where the two passes agree on the extents but not on the openers are
now handled by `Lexer::brace_scan`; this row is one where they disagree on the
extents, and no flag on the expansion's lex can express it.

**What the proper fix looks like.** `Shell::brace_extent_scan` has to run over
the brace's **text**, with the scan's own carve, rather than over the parsed
part. Concretely: keep the undecoded source of an unread `${ … }` on the part
(or reach it through `crate::unparse`), and lex it once in
`Lexer::brace_scan` mode with the single-quote rule the scan really has — a `'`
consumes to its mate and offers nothing inside, so a `$(` in there can neither
be read nor run past the mate. `read_word_verbatim` already computes that mate
(`sq_close`); what it does not do is let it bound a substitution, because for
the *expansion* it must not.

Note that `Lexer::brace_scan` as it stands is deliberately the narrow version:
it adds openers and leaves extents alone. Widening it to bound a `$( … )` at
`sq_close` would be wrong for the same lexer's expansion duty, so the widening
has to come with the second pass, not instead of it.

**Impact.** Diagnostics only — the value is already right. Reachable only
through `@P`/`PS4`/here-doc text holding a `${ … }` whose operand has both an
unterminated `$( … )` inside a `' … '` run and a failing one after it.
