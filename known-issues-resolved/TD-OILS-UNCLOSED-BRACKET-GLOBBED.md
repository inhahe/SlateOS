### TD-OILS-UNCLOSED-BRACKET-GLOBBED. a lone `[` was a glob pattern, so `[ 1 -lt 2 ]` read the whole directory — ✅ **FIXED** — 2026-08-02

**Where:** `userspace/oils/src/interp.rs` — `field_has_glob_meta`.

`field_has_glob_meta` called any unquoted `[` a metacharacter. bash's
`unquoted_glob_pattern_p` does not: a `[` only makes a word a pattern once a `]`
arrives with one still open, and the `[` is forgotten at every `/`, because a
pattern is only ever matched against one path component. So `[`, `[abc`, `a]b`
and `[a/b]` are literal words in bash and were patterns in osh.

**It was a correctness bug and a performance bug at once.**

* *Correctness:* under `nullglob` a pattern that matches nothing is deleted,
  so `echo [` and `echo [abc` printed nothing where bash prints the word.
* *Performance:* `[` is the name of a builtin, so every `[ … ]` test in every
  loop in every script began by reading the entire current directory to
  discover that `[` matched nothing. The directory's size leaked into the
  shell's speed.

Measured on the release build, 50 000 iterations of `[ 1 -lt 2 ]`:

| | before | after | bash 5.2.37 |
|---|---|---|---|
| in this repo's root (many entries) | 10 996 ms | 679 ms | 387 ms |
| in an empty directory | 3 798 ms | — | — |
| `while [ $i -lt 300000 ]; do i=$((i+1)); done` | 65 797 ms | 4 493 ms | 2 602 ms |

That is 16× on the loop body and 14.6× on the whole loop, and it takes osh from
26× bash to 1.7×. Pinned by
`tests/corpus/an-unclosed-bracket-is-a-word-not-a-pattern.sh` and
`an_unclosed_bracket_is_a_literal_word_not_a_pattern`.
