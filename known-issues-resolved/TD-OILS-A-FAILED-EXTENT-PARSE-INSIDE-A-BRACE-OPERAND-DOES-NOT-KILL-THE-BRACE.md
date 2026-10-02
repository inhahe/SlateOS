### TD-OILS-A-FAILED-EXTENT-PARSE-INSIDE-A-BRACE-OPERAND-DOES-NOT-KILL-THE-BRACE. `v='A${y:-p$(fi)q}B'; echo "${v@P}"` expands the brace; bash calls the whole thing a bad substitution — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/parser.rs`, `dquote_word_from_source` — which lexes
the whole string into parts *before* anything is expanded, so a `${ … }` finds
its `}` at lex time and the body's parse failure is only discovered later, in
`Shell::command_sub_body_inner`.

**Reproduce** (prompt expansion only — `${x@P}`, `PS1`, `PS4`):

| value | bash | osh |
|---|---|---|
| `A${y:-p$(fi)q}B` | `A${y:-p$(fi)q}B` + `bad substitution` | `AYB` |
| `A${y:+m$(fi)n}B` | `A${y:+m$(fi)n}B` + `bad substitution` | `Amn` |
| `A${y/$(fi)/z}B` | `A${y/$(fi)/z}B` + `bad substitution` | matches ✓ |

(with `y=Y`. bash also emits the extent read's own
`command substitution: … syntax error near unexpected token `fi'` first.)

**Why.** This is the same mechanism as
TD-OILS-A-FAILED-EXTENT-PARSE-CONSUMES-THE-REST-OF-THE-STRING, one level in.
`extract_dollar_brace_string` carries `SX_COMMAND`, so on meeting the `$(` it
calls `extract_command_subst` *during its own scan*. That read fails and leaves
the index at the end of the string, so the brace scan resumes past everything
and never finds its `}`. Under `no_longjmp_on_fatal_error` its EOF branch sets
`*sindex` past the text and returns NULL instead of jumping, and `param_expand`
turns the NULL into its own `bad substitution` naming the whole text — after
which `expand_prompt_string` returns NULL and the caller keeps the *undecoded*
string. Hence the text coming back unchanged.

osh cannot reach that shape because its scan and its body-parse happen at
different times: by the time the `$(fi)` is known to be unparseable, the brace
has long since been matched and turned into a `WordPart`. Note the third row
matches anyway — a pattern operand takes a different path that already fails.

**It is a *scan*-time failure, not an expansion-time one — measured 2026-08-09,
and this rules out the cheap fix.** With `y=Y` set, `${y:-…}` never evaluates its
operand, yet bash reports all the same:

```text
y=Y; a='A${y:-p$(fi)q}B'; printf '1 [%s]\n' "${a@P}"
    command substitution: line 5: syntax error near unexpected token `fi'
    command substitution: line 5: `fi)q}B'
    line 4: A${y:-p$(fi)q}B: bad substitution
    1 [A${y:-p$(fi)q}B]
```

So the obvious-looking fix — reusing `Shell::extent_consumed` and letting the
*enclosing* `${ }` observe it — cannot work: the operand is never expanded, so
nothing on the expansion path runs at all. (I tried to take this shortcut and
the measurement refused it.) Two further details the same probe pins:

- **The body never runs.** Only *one* `command substitution:` pair appears, where
  the top-level shapes produce two. `param_expand` returns NULL from the failed
  brace scan before it ever reaches `command_substitute`.
- **The text handed to the extent read is the rest of the *whole string*,** not
  of the operand: `fi)q}B` runs out through the `}` and the trailing `B`. That is
  exactly `src + ")" + tail` with the `tail` that `unparse::attach_comsub_tails`
  already computes, since it renders the whole word around the part.

**Fixed** by `Shell::brace_extent_scan` (`userspace/oils/src/interp.rs`) — but
*not* by the post-lex pass sketched below, and the write-up above was wrong in
several places. Recording both, because the corrections are the interesting part.

**What the plan above got wrong.**

- It was scoped to `dquote_word_from_source`, i.e. to prompt expansions and
  `CmdSubBody::Unread` bodies. That is far too narrow. **Every** word reaches the
  same scan, and a body the parser *did* read fails it just as readily when its
  re-print will not parse back: `y=Y; echo "A${y:-p$(`↵`!`↵`)q}B"` is a plain
  command, and bash reports `` `! )q}B"' `` and exits 1. A `CmdSubBody::Parsed`
  needed handling too.
- It wanted the scan modelled as a lex-time rewrite into
  `Unclosed::BadSubst`. Wrong shape: the failure is a *runtime* one (the extent
  read runs commands' parsers, honours `errexit`, and its jump depends on
  `Shell::prompt_expanding`), so it belongs on the expansion path after all —
  just *before* the operand rather than inside it.
- It said the "reports but does not run" mark was the one genuinely new thing.
  It was not needed: a brace-level scan failure simply returns early, and the
  body is never handed to `command_sub` at all.
- The `${y/$(fi)/z}` row in the table above is marked "matches ✓". **It does
  not.** osh produces the right *text* but emits no diagnostics whatsoever,
  because a pattern operand is parsed eagerly — see
  TD-OILS-A-BRACE-PATTERN-OPERAND-IS-PARSED-EAGERLY.

**What the fix actually is.** `brace_extent_scan(part)` walks the substitutions
bash's `extract_dollar_brace_string` would meet between the braces and runs the
extent read on each, *before* the substitution is expanded. It runs at every door
into an expansion, because there are three and only one of them is the obvious
one:

- `Shell::expand_dynamic_with` — the general one;
- `Shell::split_items` — the unquoted list recogniser, which reaches
  `expand_param_op` directly for `${x:-w}` and its relatives;
- `Shell::quoted_per_element_parts` — the same recogniser inside double quotes.

Missing the latter two is what made `"${y:-…}"` and `${y:-…}` silently skip the
scan while `${y:=…}` — which has no list form and so falls through to
`expand_dynamic_with` — behaved correctly. A scan that *succeeds* has no effect
at all, so the recognisers' `None` answers being scanned a second time on the way
through `expand_dynamic_with` is harmless.

**Two things the scan steps over,** both measured against bash 5.2.37 and both
absent from the original write-up:

- **A `' … '` run.** `skip_single_quoted` (subst.c:1926-1938) steps over one, so
  `A${y:-'p$(fi)q'}B` reports *nothing* — even though the quotes are not quotes to
  the expansion, which happily runs `A${nope:-'p$(echo Q)q'}B` into `A'pQq'B`. The
  scan and the expansion disagree in plain sight. Modelled by threading a
  single-quote toggle through `Shell::brace_scanned_subs_in`, because in unread
  double-quote text the lexer emits `Literal("'")` parts rather than a
  `SingleQuoted` part.
- **A `[ … ]` subscript, at any depth** (`skipsubscript`, subst.c:1940-1946) —
  see `unparse::Nested::Index`. A `" … "` run, by contrast, goes to
  `skip_double_quoted`, which *does* read a `$(`.

**Corpus:**
`a-failed-extent-parse-inside-a-brace-operand-does-not-kill-the-brace.sh`.

**Found by** the measurement pass for
TD-OILS-A-FAILED-EXTENT-PARSE-CONSUMES-THE-REST-OF-THE-STRING.
