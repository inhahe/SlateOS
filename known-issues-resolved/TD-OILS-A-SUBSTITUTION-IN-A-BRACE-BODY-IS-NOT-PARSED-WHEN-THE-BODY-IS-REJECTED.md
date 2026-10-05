### TD-OILS-A-SUBSTITUTION-IN-A-BRACE-BODY-IS-NOT-PARSED-WHEN-THE-BODY-IS-REJECTED. `echo ${#x:-$(fi)}` says `bad substitution` — 2026-08-07 — ✅ FIXED 2026-08-07

**Where:** `userspace/oils/src/lexer.rs` — `read_dollar_brace`; `userspace/oils/src/parser.rs` — `seg_to_part`'s `Seg::ParamBraced` arm and `parse_braced_param_in`'s `BadSubst`/`BadTransform` returns.

**What.** bash parses a nested `$( … )` **eagerly, as it reads the `${ … }` body**
— parse.y:3954 hands the `$(` to `parse_dollar_word`, which reaches
`parse_comsub` — exactly as it does under `P_ARITH` (parse.y:3937 → 3959, the
already-fixed `arith_comsubs` case). So the nested body's syntax error happens
*before* anything judges the enclosing `${ … }`, and beats every verdict that
judgement could reach:

```text
                                bash                        osh
${#x:-$(fi)}         syntax error near `fi'      bad substitution
${x@Z$(fi)}          syntax error near `fi'      bad substitution
${1[0]$(fi)}         syntax error near `fi'      bad substitution
${!x@Q$(fi)}         syntax error near `fi'      bad substitution
${$(fi)}             syntax error near `fi'      bad substitution
${@[0]$(fi)}         syntax error near `fi'      bad substitution
if false; then echo ${#x:-$(fi)}; fi
                     syntax error near `fi'      ok
```

It beats a runtime `bad substitution` (rows 1–4), it beats an outright parse
refusal of the body's shape (rows 5–6), and it fires in an **untaken branch**
(row 7) — because it is a parse-time event, not an expansion-time one.

A **backtick is not** parsed eagerly, and the difference is visible in both
directions: ``echo ${#x:-`fi`}`` gives bash a runtime `bad substitution` (the
backtick body is never read), and in an untaken branch it is silent. This is the
same asymmetry already recorded for the arithmetic scan.

A well-formed body is still only *text*: it is parsed here and **run** at
expansion, once — `n=0; inc() { n=$((n+1)); }; echo ${x:-$(inc)}` leaves `n=1`,
not 2.

**Fixed by** recording the nested `$( … )` bodies during the `${` scan and
parsing them where the enclosing body is *not* parsed:

- `Seg::ParamBraced` gained a third field, `Vec<CmdSubSpan>` — the substitutions
  met while the body was read. `read_dollar_brace` now returns it alongside the
  raw text, collecting it in the same `arith_comsubs` field the `$(( … ))` scan
  already uses, with the same save/restore at every `$( … )` body boundary (a
  scan whose text is re-lexed downstream drops the whole collection, because that
  re-lex will parse it again).
- A nested `${ … }` hands its collection **outwards** (`arith_comsubs.extend`),
  because a body one level in is read by `parse_matched_pair` too — nothing in it
  is deferred by the nesting, so `echo ${#x:-${y:-$(fi)}}` is fatal.
- The `"` arm of `read_dollar_brace_body` was a 20-line dumb copy loop that saw
  nothing inside the quotes; it now routes through `read_opaque_span`, the same
  scan every other grouping construct uses (a `"` is a `shellquote` to
  `parse_matched_pair` wherever it stands, parse.y:3844). That is what makes
  `echo ${#x:-"$(fi)"}` a syntax error rather than a `bad substitution`.
- In `seg_to_part`, the collection is parsed only when the `${ … }` **deferred
  its body** — the new `defers_its_body` predicate: an `Err` return, `BadSubst`,
  `BadTransform`, or an `ArrayBulk` with a `BulkOp::BadTransform`. On every other
  path the body's substitutions are parsed with the operand words, and parsing
  here as well would gather a nested here-document twice.

A backtick body still is not parsed eagerly, and neither is anything inside a
`'…'`; both fall out of `read_opaque_span` unchanged. Corpus case:
`tests/corpus/a-substitution-in-a-brace-body-is-parsed-where-it-is-read.sh`.
