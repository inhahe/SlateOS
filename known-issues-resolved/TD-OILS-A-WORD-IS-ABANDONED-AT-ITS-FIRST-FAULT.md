### TD-OILS-A-WORD-IS-ABANDONED-AT-ITS-FIRST-FAULT. A second diagnostic came out of a word whose expansion had already failed — 2026-08-06 — ✅ FIXED 2026-08-06

**Where:** `userspace/oils/src/interp.rs` — `Shell::raise_unbound` (new
`expansion_failed()` guard), the `ErrorIfUnset` / `AssignDefault` arms, and
`Shell::indirect_pointer_value`.

**What.** bash's `call_expand_word_internal` propagates the first expansion
failure straight out of the word list, so a word that faults is abandoned
exactly where it stood: nothing later in it is expanded, no operator is applied
to it, and no second diagnostic comes out. Only one complaint ever comes from
one command.

A bad *subscript* is the case that shows it, because the reference it belongs to
is unset by construction and would otherwise have a `set -u` complaint of its
own to add. `set -u; echo "${nope[0+]}"` reports the arithmetic syntax error at
`0+` and stops — osh went on to add `nope[0+]: unbound variable` behind it. The
same held through every operator: `:?` raised its message, `:=` evaluated the
subscript a second time to assign through it, and a `$( … )` later in the word
ran.

`${!name[sub]}` is the exception that proves the rule: the subscript is
evaluated *first*, before bash discovers there is no pointer to indirect
through, so its side effects happen and its faults are reported **in place of**
`invalid indirect expansion`. `indirect_pointer_value` now pre-evaluates the
subscript to match.

Found while corpus-locking the fatal-abort-status work above. Locked by
`userspace/oils/tests/corpus/a-word-is-abandoned-at-its-first-fault-and-says-nothing-more.sh`.
