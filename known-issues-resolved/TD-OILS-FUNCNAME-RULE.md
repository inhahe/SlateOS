### TD-OILS-FUNCNAME-RULE. A function name is held to a stricter rule than bash's, and a bad one is a syntax error — 2026-07-28 — RESOLVED 2026-07-28

**Where:** `userspace/oils/src/parser.rs`, wherever a `WORD ( )` function
definition is recognised.

**What:** bash's grammar accepts *any* word before `()`, then rejects a name it
does not like at run time — printing ``line N: `NAME': not a valid identifier``
with the word spelled as written, leaving the function undefined, and carrying
on with the rest of the script. osh instead refused to parse, so the whole
script died with ``syntax error near unexpected token `('``.

The rule bash applies is also looser than a shell identifier: the name must be
a single *unquoted, unexpanded* literal, but its characters may be nearly
anything. `a.b() { … }` and `1f() { … }` both define functions; `"f"()`,
`'f'()`, `\f()`, `f\g()` and `$x()` are the ones rejected. This mattered in
practice: `my-func() { … }` is a common real-world spelling that osh could not
parse at all.

**Fixed:** the POSIX-form gate is now "any `Tok::Word` that is not an
assignment word", and `FunctionDef` carries a `definable: bool` recording
whether the name was written as a bare word (`bare_word_here()` — a single
unquoted `Seg::Lit` — is exactly bash's "no `W_QUOTED`, no `W_HASDOLLAR`"
test). `name` holds the literal when definable and the source spelling from
`token_display()` otherwise, so the error quotes the word back as typed. The
check itself moved to `interp.rs`'s `Command::Function` arm, ahead of the
`readonly -f` check. `parse_function_keyword` got the same treatment, minus the
assignment exclusion — the lexer only forms an assignment word at the start of
a command, so `function f=g { …; }` really does define `f=g`. Covered by
`tests/corpus/function-name.sh` and `parser::tests::function_name_is_any_word`.

**Reproduce (now matching):**

```sh
my-func() { echo hi; }; my-func   # bash: hi        osh (before): syntax error near `('
a.b() { echo hi; }; a.b           # bash: hi        osh (before): syntax error near `('
\f() { echo hi; }                 # bash: `\f': not a valid identifier (rc 1, script continues)
```

**Left over:** a name beginning `?`, `*`, `+`, `@` or `!` immediately before the
`(` — `f?() { …; }` — is still a syntax error in osh, because the lexer reads
`?(` as an extglob group unconditionally. That is TD-OILS-EXTGLOB-UNGATED, not
this issue.
