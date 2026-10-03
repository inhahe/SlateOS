### TD-OILS-EXPORT-EQUALS-IN-A-FLAG-WORD. `export` alone read an `=` in a flag cluster as proof the word was an operand — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — `Shell::builtin_export`, the
`a.starts_with(b"-") && a.len() > 1 && !a.contains(&b'=')` guard on its flag loop.

**What:** a word is an option word because of its leading `-`, not because of
what follows. getopt has no notion of an assignment, so an `=` in a flag cluster
is just another letter, and an unknown one. `export`'s scan carried an exception
for it that no sibling builtin had:

```sh
export -x=1   # bash: export: -x: invalid option + synopsis, rc 2
              # osh:  export: `-x=1': not a valid identifier, rc 1
export -n=1   # bash: export: -=: invalid option — `-n` is fine, `=` is not
export -q=1   # export: -q: invalid option   (first bad letter wins)
export -=1    # export: -=: invalid option
```

Only the **leading** words are scanned, so `export a=1 -x=2` and
`export -- -x=1` correctly report `` `-x=2' ``/`` `-x=1': not a valid identifier ``;
and `+x=1` is an operand to `export`/`readonly`, not a flag word.

**Fixed 2026-08-04.** The `!a.contains(&b'=')` clause was dropped.
`readonly`'s scan never had it and already agreed, as did the declare family, so
this brought the last of the five into line. Pinned by the corpus case
`an-equals-in-a-flag-word-is-just-another-letter.sh` and the unit test
`export_reads_an_equals_in_a_flag_word_as_a_letter`.
