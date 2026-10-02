### TD-OILS-ESCAPED-KEYWORD. A backslash-escaped reserved word is still read as the keyword — 2026-07-28 — RESOLVED same day

**Where:** `userspace/oils/src/lexer.rs`, the two escape sites in
`read_word_inner` and the parameter-expansion pattern reader.

**What:** quoting *any* character of a word stops it being read as a syntactic
name, so bash runs an external `if` for `\if` and fails to find one. osh still
saw the keyword, because an escaped *alphanumeric* was deliberately folded into
the neighbouring `Seg::Lit` — leaving nothing to distinguish `\if` from `if`.
(Escaped punctuation already became its own `Seg::Sq`, and double quotes
already survived, so this was specific to backslash-escaped letters/digits.)

Found while making bare `time` a keyword: `\time true` runs the *external*
`time` in bash, but osh timed a null command.

**It was never only keywords.** The same fold broke four other readings that
key off a single flattened `Seg::Lit`: `\a=1` was taken as an assignment
(bash: a command named `a=1`), `\f() { … }` as a function definition, and
`declare -f` printed both `\ls` and `${x#\a}` back without their backslashes,
emitting a body that no longer meant what was written.

**Fix:** drop the alphanumeric carve-out at both sites, so every escaped
character becomes its own one-char `Seg::Sq`. The single-`Lit` patterns then
say no for free, and `word_src` prints the backslash back because the segment
records it. The remaining carve-out — an extglob group body, which is
accumulated as one contiguous literal — is unchanged. Covered by
`tests/corpus/escaped-word.sh`.

**Reproduce (now matching):**

```sh
\if true; then echo t; fi     # bash: syntax error near `then'   osh (before): t
\while :; do break; done      # bash: syntax error near `do'     osh (before): silent
\time true                    # bash: time: command not found    osh (before): timed it
```

**Left over:** an *invalid* function name is still a syntax error in osh where
bash accepts the parse and rejects the name at runtime — see
TD-OILS-FUNCNAME-RULE.
