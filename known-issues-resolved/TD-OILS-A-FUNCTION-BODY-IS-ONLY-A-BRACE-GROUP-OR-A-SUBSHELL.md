### TD-OILS-A-FUNCTION-BODY-IS-ONLY-A-BRACE-GROUP-OR-A-SUBSHELL. `f() if true; then :; fi` is a syntax error where bash defines a function — 2026-08-08 — ✅ FIXED 2026-08-08

**Where:** `userspace/oils/src/parser.rs` — `parse_compound_body`, which accepts
a `{ … }` brace group and a `( … )` subshell and nothing else. Both function
forms (`parse_command`'s `WORD ( )` branch and `parse_function_keyword`) go
through it.

**What.** bash's production is `function_body: shell_command`, and
`shell_command` is *every* compound command — `for`, `case`, `while`, `until`,
`select`, `if`, a subshell, a group, an arithmetic command, a conditional, and
the arithmetic `for`. osh admits two of the eleven. Everything else is rejected
at the word that opens it:

```text
f() if true; then echo hi; fi; f       bash: hi          osh: near `if'
f() for i in a b; do echo $i; done; f  bash: a b         osh: near `for'
f() while false; do :; done; f         bash: rc=0        osh: near `while'
f() until true; do :; done; f          bash: rc=0        osh: near `until'
f() select i in a; do break; done      bash: 1) a …      osh: near `select'
f() case x in x) echo m;; esac; f      bash: m           osh: near `case'
f() [[ a ]]; f                         bash: rc=0        osh: near `[['
f() ((1)); declare -f f                bash: f () { ((1)) }
                                                         osh: near `word'
```

The `function` keyword spelling fails the same way once it gets that far, with or
without the optional `()` — `function f if true; then echo hi; fi` and
`function f () ((1))` both define in bash. `function f ((1))` additionally needs
the `last_read_token == WORD && token_before_that == FUNCTION` lookbehind, which
belongs to the sibling entry above.

The set really is `shell_command` and no wider: measured, a nested definition
(`f() f2() { :; }` → near `f2`), a negation (`f() ! true` → near `!`) and a
simple command (`f() echo hi` → near `echo`) are all still errors.

**And the redirection list belongs to the body, not the function** — except when
the body is a brace group. Measured:

```text
f() { :; } >/dev/null      bash: f () { :\n} > /dev/null       osh: same
f() ( : ) >/dev/null       bash: f () {\n    ( : ) > /dev/null\n}
                                                                osh: } > /dev/null
f() ((1)) >/dev/null       bash:     ((1)) > /dev/null
f() if true; then :; fi >/dev/null
                           bash:     fi > /dev/null
```

So the subshell row is a divergence osh already has, independent of the missing
arms: `FunctionDef::redirects` — which `unparse` prints after the closing `}` —
is right only for the brace group. Everything else carries its own list.

**Impact.** A valid function definition rejected outright, and the shell stops
reading the unit that holds it.

**Fixed.** The compound-command arms are factored out of `parse_command` into
`parse_shell_command`, which returns the bare command *without* consuming a
redirection list and answers `Ok(None)` where none of the eleven starts. Both
`parse_command` and `parse_compound_body` call it, so the grammar's one
production is now written once — which is the point: the two were never meant to
be different lists, and osh's second copy had drifted to two entries.

The brace group keeps its two special properties, and keeps them by being taken
*before* the shared reader: it unwraps to the body's own `Program` (bash's
`make_function_def` takes the group's command directly, so `declare -f` re-prints
one pair of braces, not two), and its redirections stay on
`FunctionDef::redirects`, which is what prints them after the closing `}`. Every
other body now applies `with_redirects` to the command itself and wraps it as a
single-item `Program` — the subshell arm already wrapped that way, so its scope
is still not leaked, and the wrap now also carries the redirection list bash puts
inside the braces.

The stray-keyword arm (`f() then`) moved into the shared reader unchanged: a
reserved word is not a `WORD`, so it is an error in both of the production's
positions and needed no second spelling.

`declare -f` was measured against bash for all eleven bodies and both `function`
keyword spellings before the change, and osh matched every one of them the first
time the new arms compiled — `unparse` had simply never been reachable for nine
of them.

**Pinned by** the unparse test `a_function_body_is_a_whole_shell_command`, which
asserts bash's `declare -f` byte for byte for each arm and for the redirection's
two owners, and the corpus case
`a-function-body-is-a-whole-shell-command.sh`, which also runs each definition
and checks the six shapes that stay syntax errors.
