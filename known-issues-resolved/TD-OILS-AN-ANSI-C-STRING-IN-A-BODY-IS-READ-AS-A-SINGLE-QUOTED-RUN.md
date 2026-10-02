### TD-OILS-AN-ANSI-C-STRING-IN-A-BODY-IS-READ-AS-A-SINGLE-QUOTED-RUN. `$(echo $'a\'b')` fails to lex — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/lexer.rs` — `read_opaque_span` and
`read_dollar_brace_body`.

**What.** Every scanner that copies a body as *raw text* — the `$( … )` /
`$(( … ))` / `$[ … ]` balanced read and the `${ … }` read — treated a `$'…'` as
an ordinary single-quoted run. But `$'…'` honours escapes, so its `\'` does not
end it. Read as a plain quote, the run ended at the backslash and the `'` after
it opened another that never closed, taking the whole construct down:

```text
echo "$(echo $'a\'b')"        bash: a'b     osh: unexpected EOF … matching `''
echo "${q:-$'a\'b'}"          bash: (see below)
echo "[${q/$'a\'b'/Y}]"       bash: [zYz]   osh: unexpected EOF … matching `''
```

bash reads one with `P_ALLOWESC` — the `$'…' inside group` case of
`parse_matched_pair` (parse.y:3847) — and that case is reached for *every*
grouping construct, gated on neither `P_COMMAND` nor `P_ARITH`. The backtick
reader was already right by accident: bash calls `parse_matched_pair` with
`open == close` for `` ` ``, which skips the `shellquote` branch entirely, so a
`'` inside backticks is not a quote to that scan either.

**Fixed by** `Lexer::take_ansi_c_run`, which copies the run — closing quote
included — with a `\` covering whatever follows it, and a `$'` arm at both
sites that calls it. The escapes travel *unresolved*: only the extent is wanted,
because the text is being copied for a re-lex that translates them itself.

Corpus case:
`userspace/oils/tests/corpus/an-ansi-c-string-in-a-body-is-not-a-single-quoted-run.sh`.
