### TD-OILS-HISTEXPAND-PERCENT-DESIGNATOR. `!%` and `:%` (the last `?str?` match) are unimplemented — ✅ RESOLVED 2026-07-30

**Where:** `userspace/oils/src/histexpand.rs` — `apply_word_designator()` has no
`%` arm, and nothing records what a `!?string?` search matched.

**What.** bash remembers the *word* that the most recent `?string?` event search
matched, and `%` — either as the whole event (`!%`) or as a word designator
(`!!:%`) — expands to it. With no prior search it expands to nothing.

```
$ first alpha beta ; : !?lph? ; echo "[!!:%]"
bash → [alpha]        osh → [!echo …:%]   (literal, then a bogus re-scan)
$ echo !%
bash → (empty line)   osh → !%: event not found
```

**Fixed (2026-07-30).** `LastSubst` grew from "the last `:s`" into `HistState`,
the state one expansion leaves for the next — the substitution pair plus the
`?string?` search *string* and the word it *matched*. The `?` arm of `expand_one`
records both when the search succeeds; `apply_word_designator` takes the matched
word and has a `%` arm that returns it. `%` needs no range check and cannot fail,
because it does not look at the event in hand at all.

Widening the probe past what the entry above described turned up six rules, none
of them in the manual, all measured against bash 5.2:

* **The match is the word holding the *last* occurrence of the search string**,
  because readline scans the matched line backwards: `!?a?` against `xa ya za`
  remembers `za`, not `xa`.
* **It is a word of readline's tokenizer**, so an operator qualifies: `!?; b?`
  against `alpha; beta` remembers `;`, and `!?(bet?` against `alpha (beta)`
  remembers `(`.
* **Whitespace is in no word**, so a match that starts on it is remembered as
  nothing: after `!? bet?`, `%` is empty.
* **`%` ends the designator** exactly as `$` does (`!!:%*` is `gamma*`), and is
  no kind of range endpoint — `!!:1-%` is the range `1-` (whose end defaults to
  the second-to-last word) followed by a literal `%`.
* **Only a `?string?` search records anything.** A `!string` prefix search does
  not, and neither does a `^old^new^` quick substitution; both leave the previous
  match in place. A search that finds nothing leaves it in place too, while one
  that succeeds and *then* fails on its word designator (`!?eps?:9`) still
  records — the word is taken when the event is found and nothing rolls it back.
* **An empty search string means the previous search string**, as an empty
  `:s//new/` pattern does: after `!?gam?`, `!??` searches for `gam` again and so
  can find an *older* event than the most recent. With no previous search there
  is nothing to reuse, and readline refuses to search for nothing even though
  every event contains the empty string — `!??` and `!?` both fail, where osh
  matched the most recent event. That rule shares the `?` arm with the search
  record, so it is fixed in the same commit; its diagnostic quotes back what was
  *written* (`!??`, and `!?zzz` without a `?` the designator never had), which
  osh was also getting wrong for the unterminated form.

**Tests.** `a_percent_designator_names_the_word_the_last_search_matched` (the
`search_match` word-picking rules directly, then sequences of lines expanded
against one `HistState` so the cross-line memory is exercised) and
`tests/corpus/histexpand-percent.sh`. The corpus case has one layout constraint
worth knowing: `history -s` deletes the last history entry before adding its
argument, and that entry is the just-read probe line — which is what keeps a
leftover probe line, whose text necessarily *contains the string it searched
for*, from being found in preference to the intended event.
