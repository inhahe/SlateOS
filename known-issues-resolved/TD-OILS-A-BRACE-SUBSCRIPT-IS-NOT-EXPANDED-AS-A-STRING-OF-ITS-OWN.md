### TD-OILS-A-BRACE-SUBSCRIPT-IS-NOT-EXPANDED-AS-A-STRING-OF-ITS-OWN. `${a[$(fi)]}` names the whole word where bash names only the subscript — 2026-08-09 — ✅ FIXED 2026-08-09

**Where:** `userspace/oils/src/interp.rs`, the extent read behind
`Shell::extent_consumed` and `unparse::attach_comsub_tails`, which have no
notion of a subscript being its own string scope.

The *parse* half is fixed: `split_name_subscript` now takes the enclosing
`Quoting` and passes `q.as_pattern()`, so a subscript in unread text holds a
`CmdSubBody::Unread` and the reads happen at all (see
TD-OILS-A-BRACE-PATTERN-OPERAND-IS-PARSED-EAGERLY). What is left is the scope
they run at.

**Reproduce** (`declare -a a=(0 1 2)`, `y=Y`, expanded with `@P`):

```text
A${a[$(fi)]}B
  bash: command substitution: line 4: syntax error near unexpected token `fi'
        command substitution: line 4: `fi)'
        command substitution: line 3: syntax error near unexpected token `fi'
        command substitution: line 3: `fi'
        A0B
  osh:  … same two reports, but the tails are `fi)]}B' and `fi)]}'
        A0                     ← the trailing `B` is eaten

A${y:-${a[$(fi)]}}B
  bash: AYB                    (no diagnostics — see below)
  osh:  AYB                    ✅ matches since the parse fix
```

Both defects are the same one: the reads are run against the *word's* remaining
text instead of the subscript's, so the tail names too much and
`Shell::extent_consumed` swallows the word's remainder rather than the
subscript's.

**Why.** Two separate rules meet here.

1. `extract_dollar_brace_string` **steps over** a `[ … ]` at any depth
   (`skipsubscript`, subst.c:1940-1946), so the brace scan never reads the
   `$( … )` inside one — which is why the second row is silent even though the
   `$( … )` is unparseable. See `unparse::Nested::Index`, which already models
   this for `Shell::brace_scanned_subs`.
2. The subscript text is later expanded as a **string in its own right**, and the
   ordinary string-level rule applies there: one report from the extent read and
   one from the body's real run (see
   TD-OILS-A-FAILED-EXTENT-PARSE-CONSUMES-THE-REST-OF-THE-STRING). The tail is
   the subscript's own remainder (`` `fi)' ``), not the whole word's — the
   subscript is its own scope. The arithmetic evaluation of the empty result
   then yields index 0, hence `A0B`.

**Proper fix.** The subscript's expansion has to run the extent read at its own
scope, with the subscript's remainder as the tail.
`unparse::attach_comsub_tails` already computes a per-part tail by rendering the
enclosing container, so the piece to add is a container boundary at the
subscript rather than new tail machinery — and `Shell::extent_consumed` needs the
same boundary, so an `Abandoned` read inside a subscript consumes the subscript
and not the word.

**Fixed** exactly as sketched, in two independent halves.

*The tail.* `unparse::attach_comsub_tails` was split into a per-scope
`attach_comsub_tails_in`, and a new `index_scopes` walks into every
`Nested::Index` and runs that scope separately, while `walk_parts_in` now skips
an `Index` so the enclosing scope never sees the parts inside one. Each scope
therefore renders only its own text, and the tail a report quotes is the
subscript's remainder. The retry's one-byte-shorter tail falls out of the
existing `ep - base - 1` modelling, so `A${a[p$(fi)q]}B` gives `` `fi)q' `` then
`` `fi)' `` and three reports come out of `${a[$(fi)+$(fi)]}` in the right order.

*The consumption.* Three walks are bash's `expand_word_internal` and so own a
`sindex`: `Shell::expand_double_quoted` (already did), `expand_to_arith_string`
and `expand_word_joined_annotated` / `expand_word_annotated` — the last two now
save-and-clear `Shell::extent_consumed` around their part loop and break out of
it when the flag comes back set. `expand_word_annotated` asks at the *top* of
the loop because its arms reach the next part by `continue` as often as by
falling off the end. A new `Shell::expand_subscript_key` names bash's
`expand_subscript_string` at the ten associative-key call sites; it adds nothing
to the word walk's scope but the name.

That second half is what makes an associative subscript report at all:
`A${m[$(fi)qq]}B` leaves an **empty** key, which is `m: bad array subscript`,
and expands to `AB`. An indexed one leaves an empty arithmetic string, which is
0 — `A${a[$(fi)]}B${a[1]}C` is `A0B1C`, the word after the subscript untouched.

**Corpus:**
`a-brace-subscript-is-a-string-of-its-own-and-eats-its-own-tail.sh`.

**Found while fixing it**, both pre-existing and both filed separately:
TD-OILS-AN-ARITHMETIC-ERROR-UNDER-@P-STILL-ABANDONS-THE-COMMAND and
TD-OILS-AN-INDIRECT-BRACE-EXPANDS-ITS-POINTER-THE-WRONG-NUMBER-OF-TIMES.
