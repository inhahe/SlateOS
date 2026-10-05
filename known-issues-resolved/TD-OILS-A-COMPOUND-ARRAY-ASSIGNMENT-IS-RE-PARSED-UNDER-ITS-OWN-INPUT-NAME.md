### TD-OILS-A-COMPOUND-ARRAY-ASSIGNMENT-IS-RE-PARSED-UNDER-ITS-OWN-INPUT-NAME. A `$( … )` inside `a=( … )` that will not parse back is blamed on `command substitution`, not on `array assign` — 2026-08-08 — ✅ RESOLVED 2026-08-08

**Where:** `userspace/oils/src/interp.rs` — `Shell::comsub_reparse_error` and
its caller in `Shell::command_sub_body_inner`; the compound-assignment
expansion path (`exec_declare_with_arrays` / the `a=( … )` operand handling).

**What.** bash re-parses a `$( … )` re-print at expansion time
(TD-OILS-A-CMDSUB-BODY-IS-RE-READ-AS-WRITTEN-NOT-AS-REPRINTED, resolved), and
osh now does too. But a **compound array assignment** does not reach that path
in bash at all: the whole value list is re-parsed first, as a *string*, by

```c
list = parse_string_to_word_list (val, 1, "array assign");
```

(arrayfunc.c:587, from `assign_compound_array_list`). That string already holds
the substitution's re-print — parse-time `parse_comsub` put it there — so the
`$(` is met by the *tokenizer* of this second parse, and the failure is
`parse_comsub`'s own: `jump_to_top_level (FORCE_EOF)` (parse.y:4185), which
ends a non-interactive shell.

Three things differ from the ordinary re-parse, and osh gets all three wrong:

```sh
a=( "p$(
!
)q" r )
echo "after rc=$?"
```

```text
bash: a.sh: array assign: line 1: syntax error near unexpected token `)'
      a.sh: array assign: line 1: `"p$(! )q" r'
      (no `after'; exit 2)

osh:  a.sh: command substitution: line 4: syntax error near unexpected token `)'
      a.sh: command substitution: line 4: `! )q"'
      after rc=1
      (exit 0)
```

- **The input name** is `array assign`, not `command substitution`.
- **The line is 1**, always: `parse_string_to_word_list` does `push_stream (1)`
  — the argument that *does* reset `line_number`, unlike the `push_stream (0)`
  of `parse_string`, which is why the ordinary re-parse's counter runs on from
  the executing command instead.
- **The echoed line is the whole re-printed value list** (`"p$(! )q" r`), not
  the failing substitution's own line plus its word tail, because the reader is
  standing in the array-assign string rather than in the body.
- **It is fatal**, FORCE_EOF-class: nothing after it runs and the shell exits 2,
  where osh discards one parse unit and carries on.

`declare -a b=( "p$(⏎!⏎)q" )` behaves identically (same name, same line 1).

**The proper fix** is to give the compound-assignment expansion its own
re-parse step rather than letting the substitutions inside it be expanded one
at a time: render the value list back to a string (the parts already re-print
correctly — the echoed text above is exactly osh's `word_src` of the list),
push an `InputSource` named `array assign` with the line counter *reset*, parse
it as a word list, and on failure take the FORCE_EOF branch
(`FatalAbort { status: 2, demote: false }`) that
`Shell::comsub_reparse_error`'s `nested` flag already selects. The per-body
re-parse must then be suppressed inside it, or the same failure is reported
twice under two different names.

**Impact.** Confined to a compound array assignment holding a `$( … )` whose
re-print will not parse back — in practice a body ending in a bare `!` or
`time`. osh continues where bash exits, and names the wrong source.

**Found by** the probe matrix for
TD-OILS-A-CMDSUB-BODY-IS-RE-READ-AS-WRITTEN-NOT-AS-REPRINTED (probe `t12`).

**Resolved.** `Shell::array_assign_reparse_error` (interp.rs) runs before any
part of a compound assignment is expanded: it re-parses each stored `$( … )`
re-print in walk order, and on the first failure reports it under a
`SRC_TOKEN_ARRAY_ASSIGN` (`array assign`) input source, with the line counted
**from 1 inside the rendered listing** and the echoed line taken from the
listing rather than from the body — then aborts FORCE_EOF-class (status 2 at
the outermost read-eval, 1 under `eval`). The listing itself is rendered by
`unparse::array_listing`, which joins the elements with a *single space* the
way `parse_compound_assignment` (parse.y:4715) writes them back, and
`unparse::array_listing_split` cuts it at the failing substitution's closing
`)` using the same grown-NUL sentinel trick `attach_comsub_tails` uses.
Covered by
`tests/corpus/a-compound-array-assignment-is-re-parsed-under-its-own-input-name.sh`.

Four corrections to this entry, all from measurement:

- The failure is **not** always reported: a readonly name and a nameref
  designating an element are both refused *before*
  `assign_compound_array_list` is reached, so those still succeed with the
  ordinary "readonly variable" / "invalid variable name" diagnostic.
- A backtick body is a matched pair to the tokenizer, never re-parsed, so
  ``a=( "p`⏎!⏎`q" )`` assigns cleanly.
- A compound literal used as a **command prefix** never reaches
  `assign_compound_array_list` at all — it is expanded as an ordinary word, so
  `command substitution` *is* the right name there, and it is not fatal.
- The line is 1 only because the listing is one line; a newline the listing
  *kept* (one inside a quoted element, or a heredoc body in a re-print) moves
  both the number and the echoed line, so the fix counts newlines rather than
  hard-coding 1.

Fixing this also exposed a pre-existing divergence in the prefix case, fixed
in the same commit: tails were attached to a `$( … )` per *element word*, but
bash keeps the whole literal as one word, so the echo stopped at the element's
end instead of running on to the literal's closing `)`.
`unparse::attach_compound_comsub_tails`, called from the parser once the
element list is complete, re-attaches every tail across the whole listing.
