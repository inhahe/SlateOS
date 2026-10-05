### TD-OILS-OPERAND-QUOTING-IS-LOST. `${x:-'a b'}` splits and `${x:-'*'}` globs — 2026-08-04 — ✅ FIXED 2026-08-04

**Where:** `userspace/oils/src/interp.rs` — the whole unquoted-expansion
pipeline: `Shell::expand_param_op` and `Shell::array_op_fields` hand their
operand back as a plain `Str` (via `expand_to_string`), `Shell::split_items`
carries it as `SplitItems`-of-`Str`, and the IFS scan in the `other` arm of
`Shell::expand_word_annotated` then rebuilds it with
`EChar { c, quoted: false }` for every character. So by the time anything can
ask, the answer is gone.

**Reproduce** (`show() { printf '  %-12s(%d)' "$1" $(($# - 1)); shift; printf '<%s>' "$@"; printf '\n'; }`):

```sh
show 'sq'    ${nope:-'a b'}      # bash (1)<a b>     osh (2)<a><b>
show 'dq'    ${nope:-"a b"}      # bash (1)<a b>     osh (2)<a><b>
show 'esc'   ${nope:-a\ b}       # bash (1)<a b>     osh (2)<a><b>
show 'glob'  ${nope:-'*'}        # bash (1)<*>       osh globs the directory
show 'qvar'  ${nope:-"$v"}       # v='p q': bash (1)<p q>  osh (2)<p><q>
show 'empty' ${nope:-''}         # bash (1)<>        osh (0)
show 'alt'   ${x:+'a b'}         # bash (1)<a b>     osh (2)<a><b>
show 'dash'  ${nope-'a b'}       # bash (1)<a b>     osh (2)<a><b>
show 'arr'   ${a[@]:-'a b' c}    # a=(): bash (2)<a b><c>  osh (3)<a><b><c>
( set --; show 'pos' ${@:-'a b' c} )  #     bash (2)<a b><c>  osh (3)
```

It reaches the pattern contexts too, where the quoted metacharacter should be
matched literally:

```sh
[[ ab == ${nope:-'*'} ]]            # bash: no match    osh: matches
case ab in ${nope:-'*'}) ;; esac    # bash: no match    osh: matches
v='a*b'; echo "${v#${nope:-'a*'}}"  # bash <b>          osh <*b>
v='a*b'; echo "${v/${nope:-'*'}/Z}" # bash <aZb>        osh <Z>
```

**The rule.** The operand of `:-`/`:+` (and of the colon-less `-`/`+`, and of
their array and positional spellings) is substituted *as the word it is* —
fields, quoting and all. A character that came out of a quoted stretch of the
operand is not a split candidate and not a glob metacharacter, exactly as it
would not be if the operand had been written where the substitution stands.
Unquoted operand text still splits: `IFS=:; ${nope:-a:b}` is two fields, and
`${nope:-a b}` under the default `$IFS` is two — so this is not "the operand is
quoted", it is that the *quoting inside it survives*.

Three consequences worth spelling out, all measured:

- **The operand's own field breaks survive.** `a=('p q' r)`;
  `${nope:-A"${a[@]}"B}` is `(2)<Ap q><rB>` — the quoted `[@]` inside the
  operand breaks a field just as it would in a bare word, and the text around
  the substitution glues to the ends. Joining the operand into one string and
  splitting it could not produce that.
- **A quoted empty operand is a field.** `${nope:-''}` is one empty argument
  where `${nope:-$e}` (with `e=`) is none — the same "reached something / reached
  nothing" rule
  `tests/corpus/an-inactive-alternate-keeps-a-field-when-the-array-is-there.sh`
  pins for `"${a[@]:+A}"`.
- **Only these two operators keep it.** `${nope:='a b'}` splits into two fields
  in bash as well, and so do `${x/b/'a c'}` and `${x/b/'*'}` (the replacement is
  re-globbed) — so `:=` and the replacement half of `${x/p/r}` are *correct as
  they stand* and must not be "fixed" along with the rest.

**The fix.** Carry the annotation instead of rebuilding it:

1. `Shell::expand_param_op` and `Shell::array_op_fields` return
   `Vec<Vec<EChar>>` rather than `Str`/`Vec<Str>`; the operand branches get
   their fields from `expand_word_annotated(arg, false)`, which already marks
   quoted characters and already keeps a quoted `[@]`'s field breaks. Every
   other branch (the parameter's own value, an array's elements) wraps its
   bytes as unquoted.
2. `SplitItems::List`/`Joined` hold `Vec<Vec<EChar>>`; the arms that feed them
   plain element lists wrap them unquoted. `split_items` gains a `ParamOp` arm
   for `UseDefault`/`UseAlternate` so the scalar spelling reaches the same door
   as the array one.
3. The IFS scan in `expand_word_annotated` runs over `Vec<EChar>`, and `is_ws`/
   `is_nonws` answer `false` for a quoted character; the join in the
   non-null-`$IFS` case inserts an unquoted `at_sep`.
4. `expand_word_pattern_inner` takes the same annotated route for its `other`
   arm, which is what makes the `[[ ]]`, `case`, `${x#pat}` and `${x/pat/}`
   pattern cases above come out right.

Rejected: a `last_quoted` side-channel on `Shell` (hidden coupling between two
functions that otherwise only pass values), and re-expanding the operand a
second time to annotate it (it would run its side effects twice).

**Fixed** essentially as planned, with one correction the plan did not foresee.
`SplitItems` gained a third variant `Fields(Vec<Vec<EChar>>)` rather than
converting `List`/`Joined` to annotated bytes: the two kinds of list are *not*
the same thing, and flattening them together loses the difference (an empty
*element* opens no field, an empty operand *field* does). `expand_param_op` and
`array_op_fields` return `SplitItems`, `split_items` grew the `ParamOp` arm, the
IFS scan moved to a free `split_run` that answers `false` for a quoted
character, and a new `Shell::operand_chars` is the door the non-splitting
contexts (`expand_word_annotated`'s text branch, `expand_word_pattern_inner`)
knock on.

The correction: the operand's own expansion is **not** `split`-mode. It runs in
a third `SplitMode::Operand`, which splits nothing — the word the substitution
sits in does that — but keeps a list's field breaks under a null `$IFS`.
Expanding the operand with splitting on made `IFS=:; ${nope:-${a[@]}}` two
fields where bash gives one: with a non-null `$IFS` bash joins the operand's
list into one string (a space for `[@]`, `$IFS` for `[*]`) and lets the *outer*
scan find the delimiters, and only a null `$IFS` — where no delimiter is left —
leaves the breaks standing.

Two smaller corrections came out of the sweep, both from routing an answer that
used to be one string through a variant that can be several. A **quoted**
`"${a[@]:-w}"` is one field however many the operand made — the quotes are
around the word — and an operand that expands to nothing is still one *empty*
field, which is the whole difference between the two operators there
(`"${a[@]:-}"` is one empty argument, the inactive `"${a[@]:+A}"` is none). That
is `SplitItems::quoted_fields`, which the quoted per-element arm and the
string-joining `joined_value` both call; the latter needs it too, so that
`IFS=:; v=${a[*]:-'p q' r}` is `p q r` — only a list's *elements* answer to the
`[*]` separator, never an operand's fields. And the operand is expanded through
a `Shell::expand_operand_fields` that takes over `Shell::bad_sub_word`, because
it is a word-expansion entry like `Shell::expand_word`: bash recurses into
`expand_word_internal` for the operand, so `${y:-pre${x@Z}post}` names
`pre${x@Z}post` and not the substitution around it.

**Pinned by** `tests/corpus/a-substituted-operand-is-the-word-it-was-written-as.sh`,
and by the three cases that caught those two corrections —
`an-inactive-alternate-keeps-a-field-when-the-array-is-there`,
`a-whole-arrays-null-test-is-the-string-it-would-expand-to` and
`bad-substitution`. One thing the new case deliberately does not cover, which
`TD-OILS-QUOTED-EMPTY-IN-AN-OPERAND-LEAVES-NO-FIELD` below then fixed on its
own: the field a quoted-empty operand leaves.
